//! The event log: one append-only Postgres table, the source of truth for
//! what happened, and beside it the content store, `content`, holding what
//! people and sources wrote (`events::CONTENT`), where it can be deleted.
//!
//! `migrate` runs with an administrator URL and creates the tables and the
//! kernel's role (and the knowledge base's index, `knowledge::migrate_sql`).
//! On `events` the role may INSERT and SELECT and nothing else, so the
//! database itself refuses rewrites of history. Everything else the kernel
//! knows is a projection of the log and its content, rebuilt on boot.

mod content;
mod migrate;
use crate::events::Ev;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
#[cfg(test)]
use content::hash;
pub use content::{join, split, Content};
#[cfg(test)]
pub(crate) use migrate::move_inline_content;
pub use migrate::{content_walls_sql, migrate, CONTENT_OWNERS};
use serde_json::{json, Value};
#[cfg(test)]
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, NoTls};

/// How often startup asks for the log's database again while it waits.
const OPEN_RETRY_MS: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct Event {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub actor: String,
    pub ev: Ev,
}

pub async fn connect(url: &str) -> Result<Client> {
    let (client, conn) = tokio_postgres::connect(url, NoTls).await.with_context(|| "connect to Postgres")?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::error!("postgres connection: {e}");
        }
    });
    Ok(client)
}

pub(crate) fn ident(s: &str) -> Result<&str> {
    anyhow::ensure!(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "unsafe SQL identifier {s:?}");
    Ok(s)
}

/// Which events hold one item's own content: those of `runs` except the
/// kinds in `except` (what the runs made that is an item of its own); those
/// of `kinds` whose `field` is `id`; each `(kind, field, value)` of
/// `matches` (what belongs to the item but carries no run: its verdict, the
/// answers to its cards); and each `(kind, run, text)` of `containing`, the
/// events of that kind in that run whose content holds the text (where a
/// memory was written); and the events in `seqs` (the results that read
/// it into a run, `read_copies`).
#[derive(Debug, Default)]
pub struct Holding {
    pub runs: Vec<String>,
    pub except: Vec<&'static str>,
    pub kinds: Vec<&'static str>,
    pub field: &'static str,
    pub id: String,
    pub matches: Vec<(&'static str, &'static str, String)>,
    pub containing: Vec<(&'static str, String, String)>,
    pub seqs: Vec<i64>,
}

/// Maximum concurrent read-only Postgres connections for log audit/export queries.
const LOG_READ_POOL_SIZE: usize = 4;

/// The event log. A lost connection (Postgres restarting) is opened again
/// on the next use, with the writer's lock taken again; until then every
/// write fails and health says so.
pub struct Log {
    url: String,
    writer: bool,
    /// The content store exists. A release installed without `migrate`
    /// since has none yet, and keeps content in the log as before.
    store: bool,
    /// The content store records whose each row is (its `person` column).
    /// A release installed without `migrate` since writes rows without it.
    owners: bool,
    client: tokio::sync::RwLock<Client>,
    /// Dedicated read-only connection pool so audit/export/file reads never contend with appends.
    readers: crate::pg_pool::PgPool,
    /// The last event this process wrote or read, to find one that was
    /// written while its answer was lost.
    last: std::sync::atomic::AtomicI64,
}

/// One kernel per log: a second one would break the single-writer rule.
pub(crate) async fn hold(client: &Client) -> Result<()> {
    let got: bool = client.query_one("SELECT pg_try_advisory_lock(7239001)", &[]).await?.get(0);
    anyhow::ensure!(got, "another kernel holds this event log");
    Ok(())
}

impl Log {
    async fn new(url: &str, writer: bool, client: Client) -> Result<Log> {
        let store: bool = client.query_one("SELECT to_regclass('public.content') IS NOT NULL", &[]).await?.get(0);
        if !store {
            tracing::warn!("no content store: content stays in the event log until `magnus migrate` runs");
        }
        let owners: bool = client
            .query_one("SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'content' AND column_name = 'person')", &[])
            .await?
            .get(0);
        if store && !owners {
            tracing::warn!("the content store does not record whose each row is until `magnus migrate` runs");
        }
        let readers = crate::pg_pool::PgPool::new(url, LOG_READ_POOL_SIZE);
        Ok(Log { url: url.into(), writer, store, owners, client: tokio::sync::RwLock::new(client), readers, last: 0.into() })
    }

    pub async fn open(url: &str) -> Result<Log> {
        let client = connect(url).await?;
        hold(&client).await?;
        Log::new(url, true, client).await
    }

    /// The events joined to their content, as `e` and `c`.
    fn rows(&self) -> &'static str {
        match self.store {
            true => "public.events e LEFT JOIN public.content c USING (seq)",
            false => "public.events e CROSS JOIN (SELECT NULL::bytea AS salt, NULL::jsonb AS body) c",
        }
    }

    fn split(&self, ev: &Ev) -> Result<(Value, Option<Content>)> {
        let body = serde_json::to_value(ev)?;
        Ok(if self.store { split(body) } else { (body, None) })
    }

    /// Whether the log can be written: the connection is up, or opens again.
    pub async fn up(&self) -> bool {
        self.client().await.is_ok()
    }

    /// The connection, opened again if it was lost.
    async fn client(&self) -> Result<tokio::sync::RwLockReadGuard<'_, Client>> {
        {
            let c = self.client.read().await;
            if !c.is_closed() {
                return Ok(c);
            }
        }
        let mut w = self.client.write().await;
        if w.is_closed() {
            let c = connect(&self.url).await?;
            if self.writer {
                hold(&c).await?;
                // An append whose answer was lost may be in the log but not
                // in this process's state: start again from the log.
                let max: i64 = c.query_one("SELECT coalesce(max(seq), 0) FROM public.events", &[]).await?.get(0);
                if max != self.last.load(std::sync::atomic::Ordering::SeqCst) {
                    tracing::error!("the log has events this process did not apply; restarting from the log");
                    c.batch_execute("SELECT pg_advisory_unlock_all()").await?;
                    crate::update::restart_in_place();
                }
            }
            tracing::warn!("the event log's connection was lost and is open again");
            *w = c;
        }
        Ok(w.downgrade())
    }

    /// The writer's appends are serialized, so the first seq of a new
    /// append follows the last one this process applied. A gap that holds
    /// events means an append committed whose caller never applied it (its
    /// future was dropped mid-statement): start again from the log. A gap
    /// without events is only the sequence skipping a failed insert.
    async fn check_contiguous(&self, first: i64) -> Result<()> {
        let last = self.last.load(std::sync::atomic::Ordering::SeqCst);
        if !self.writer || first <= last + 1 {
            return Ok(());
        }
        let c = self.client().await?;
        let missed: i64 = c.query_one("SELECT count(*) FROM public.events WHERE seq > $1 AND seq < $2", &[&last, &first]).await?.get(0);
        if missed > 0 {
            tracing::error!("the log has {missed} events this process did not apply; restarting from the log");
            c.batch_execute("SELECT pg_advisory_unlock_all()").await?;
            crate::update::restart_in_place();
        }
        Ok(())
    }

    fn saw(&self, seq: i64) {
        self.last.fetch_max(seq, std::sync::atomic::Ordering::SeqCst);
    }

    /// Open, waiting up to `wait` for another kernel to let go of the log.
    pub async fn open_waiting(url: &str, wait: std::time::Duration) -> Result<Log> {
        let start = std::time::Instant::now();
        loop {
            match Log::open(url).await {
                Err(e) if start.elapsed() < wait => {
                    tracing::info!("waiting for the log: {e:#}");
                    tokio::time::sleep(OPEN_RETRY_MS).await;
                }
                r => return r,
            }
        }
    }

    /// Read the log without holding it, as a release about to be switched
    /// to does while the running one keeps writing.
    pub async fn read_only(url: &str) -> Result<Log> {
        Log::new(url, false, connect(url).await?).await
    }

    /// One event, and its content beside it with the person it belongs
    /// to, in one statement.
    pub async fn append(&self, actor: &str, ev: &Ev, owner: Option<&str>) -> Result<Event> {
        let (body, content) = self.split(ev)?;
        let kind = body["kind"].as_str().unwrap_or_default().to_string();
        let run = ev.run().map(String::from);
        let (salt, content) = content.map(|c| (c.salt, c.body)).unzip();
        let c = self.client().await?;
        let row = match self.store {
            true if self.owners => {
                c.query_one(
                    "WITH e AS (INSERT INTO public.events (kind, actor, run_id, body) VALUES ($1, $2, $3, $4) RETURNING seq, at),
                      c AS (INSERT INTO public.content (seq, salt, body, person) SELECT seq, $5, $6, $7 FROM e WHERE $6::jsonb IS NOT NULL)
                 SELECT seq, at FROM e",
                    &[&kind, &actor, &run, &body, &salt, &content, &owner],
                )
                .await
            }
            true => {
                c.query_one(
                    "WITH e AS (INSERT INTO public.events (kind, actor, run_id, body) VALUES ($1, $2, $3, $4) RETURNING seq, at),
                      c AS (INSERT INTO public.content (seq, salt, body) SELECT seq, $5, $6 FROM e WHERE $6::jsonb IS NOT NULL)
                 SELECT seq, at FROM e",
                    &[&kind, &actor, &run, &body, &salt, &content],
                )
                .await
            }
            false => c.query_one("INSERT INTO public.events (kind, actor, run_id, body) VALUES ($1, $2, $3, $4) RETURNING seq, at", &[&kind, &actor, &run, &body]).await,
        }
        .context("append to the event log")?;
        self.check_contiguous(row.get(0)).await?;
        self.saw(row.get(0));
        Ok(Event { seq: row.get(0), at: row.get(1), actor: actor.into(), ev: ev.clone() })
    }

    /// Several events, each with its actor (and the person its content
    /// belongs to), in one statement: all are in the log or none is.
    pub async fn append_all(&self, evs: &[(String, Ev)], owners: &[Option<String>]) -> Result<Vec<Event>> {
        let actors: Vec<String> = evs.iter().map(|(a, _)| a.to_string()).collect();
        let evs: Vec<&Ev> = evs.iter().map(|(_, e)| e).collect();
        let (bodies, contents): (Vec<Value>, Vec<Option<Content>>) = evs.iter().map(|e| self.split(e)).collect::<Result<Vec<_>>>()?.into_iter().unzip();
        let (salts, contents): (Vec<Option<Vec<u8>>>, Vec<Option<Value>>) = contents.into_iter().map(|c| c.map(|c| (c.salt, c.body)).unzip()).unzip();
        let kinds: Vec<String> = bodies.iter().map(|b| b["kind"].as_str().unwrap_or_default().to_string()).collect();
        let runs: Vec<Option<String>> = evs.iter().map(|e| e.run().map(String::from)).collect();
        let c = self.client().await?;
        // Identities are given in insert order, so the n-th seq is the n-th event.
        let rows = match self.store {
            true if self.owners => {
                c.query(
                    "WITH e AS (INSERT INTO public.events (kind, actor, run_id, body)
                            SELECT k, a, r, b FROM unnest($1::text[], $2::text[], $3::text[], $4::jsonb[]) WITH ORDINALITY AS e(k, a, r, b, n) ORDER BY n
                            RETURNING seq, at),
                      o AS (SELECT seq, at, row_number() OVER (ORDER BY seq) AS n FROM e),
                      c AS (INSERT INTO public.content (seq, salt, body, person)
                            SELECT o.seq, x.s, x.b, x.p FROM o JOIN unnest($5::bytea[], $6::jsonb[], $7::text[]) WITH ORDINALITY AS x(s, b, p, n) ON x.n = o.n
                            WHERE x.b IS NOT NULL)
                 SELECT seq, at FROM o",
                    &[&kinds, &actors, &runs, &bodies, &salts, &contents, &owners],
                )
                .await
            }
            true => {
                c.query(
                    "WITH e AS (INSERT INTO public.events (kind, actor, run_id, body)
                            SELECT k, a, r, b FROM unnest($1::text[], $2::text[], $3::text[], $4::jsonb[]) WITH ORDINALITY AS e(k, a, r, b, n) ORDER BY n
                            RETURNING seq, at),
                      o AS (SELECT seq, at, row_number() OVER (ORDER BY seq) AS n FROM e),
                      c AS (INSERT INTO public.content (seq, salt, body)
                            SELECT o.seq, x.s, x.b FROM o JOIN unnest($5::bytea[], $6::jsonb[]) WITH ORDINALITY AS x(s, b, n) ON x.n = o.n
                            WHERE x.b IS NOT NULL)
                 SELECT seq, at FROM o",
                    &[&kinds, &actors, &runs, &bodies, &salts, &contents],
                )
                .await
            }
            false => {
                c.query(
                    "INSERT INTO public.events (kind, actor, run_id, body)
                 SELECT k, a, r, b FROM unnest($1::text[], $2::text[], $3::text[], $4::jsonb[]) WITH ORDINALITY AS e(k, a, r, b, n) ORDER BY n
                 RETURNING seq, at",
                    &[&kinds, &actors, &runs, &bodies],
                )
                .await
            }
        }
        .context("append to the event log")?;
        let mut at: Vec<(i64, chrono::DateTime<chrono::Utc>)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
        at.sort_by_key(|(seq, _)| *seq);
        if let Some((seq, _)) = at.first() {
            self.check_contiguous(*seq).await?;
        }
        if let Some((seq, _)) = at.last() {
            self.saw(*seq);
        }
        Ok(at.into_iter().zip(evs).zip(actors).map(|(((seq, at), ev), actor)| Event { seq, at, actor, ev: ev.clone() }).collect())
    }

    /// One delivered file's content, read straight from the log and the store.
    pub async fn output(&self, run: &str, id: &str) -> Result<Option<(String, String, Vec<u8>)>> {
        let sql = format!("SELECT e.body, c.body FROM {} WHERE e.run_id = $1 AND e.kind = 'output' AND e.body->>'id' = $2", self.rows());
        let row = self.readers.get().await?.query_opt(&sql, &[&run, &id]).await?;
        let Some(row) = row else { return Ok(None) };
        let b = join(row.get(0), row.get(1));
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b["data_b64"].as_str().unwrap_or_default())?;
        Ok(Some((b["name"].as_str().unwrap_or("file").into(), b["media_type"].as_str().unwrap_or("application/octet-stream").into(), bytes)))
    }

    /// The content of these events, as the kernel reads it (its own role
    /// sees every row): for what the kernel guards itself, such as the org
    /// context bundle's documents, whose visibility it decides (K2).
    pub async fn contents(&self, seqs: &[i64]) -> Result<std::collections::HashMap<i64, Value>> {
        if seqs.is_empty() {
            return Ok(Default::default());
        }
        // Joined with the events, so content kept in an event (no content
        // store) reads the same.
        let sql = format!("SELECT e.seq, e.body, c.body FROM {} WHERE e.seq = ANY($1)", self.rows());
        let rows = self.readers.get().await?.query(&sql, &[&seqs]).await.context("read content")?;
        Ok(rows.iter().map(|r| (r.get::<_, i64>(0), join(r.get(1), r.get(2)))).collect())
    }

    /// The events whose content an item holds, that still have it.
    pub async fn content_seqs(&self, h: &Holding) -> Result<Vec<i64>> {
        if !self.store {
            return Ok(vec![]);
        }
        let except: Vec<&str> = h.except.clone();
        let kinds: Vec<&str> = h.kinds.clone();
        let (mk, mf, mv): (Vec<&str>, Vec<&str>, Vec<&str>) = h.matches.iter().map(|(k, f, v)| (*k, *f, v.as_str())).fold((vec![], vec![], vec![]), |mut a, (k, f, v)| {
            a.0.push(k);
            a.1.push(f);
            a.2.push(v);
            a
        });
        let (ck, cr, ct): (Vec<&str>, Vec<&str>, Vec<&str>) = h.containing.iter().fold((vec![], vec![], vec![]), |mut a, (k, r, t)| {
            a.0.push(k);
            a.1.push(r.as_str());
            a.2.push(t.as_str());
            a
        });
        let rows = self
            .readers
            .get()
            .await?
            .query(
                "SELECT e.seq FROM public.events e JOIN public.content c USING (seq)
                 WHERE (e.run_id = ANY($1) AND e.kind <> ALL($2)) OR (e.kind = ANY($3) AND e.body->>$4 = $5)
                    OR EXISTS (SELECT 1 FROM unnest($6::text[], $7::text[], $8::text[]) AS m(k, f, v) WHERE e.kind = m.k AND e.body->>m.f = m.v)
                    OR EXISTS (SELECT 1 FROM unnest($9::text[], $10::text[], $11::text[]) AS x(k, r, t) WHERE e.kind = x.k AND e.run_id = x.r AND strpos(c.body::text, x.t) > 0)
                    OR e.seq = ANY($12)
                 ORDER BY e.seq",
                &[&h.runs, &except, &kinds, &h.field, &h.id, &mk, &mf, &mv, &ck, &cr, &ct, &h.seqs],
            )
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The events whose content is a person's: everything they wrote and
    /// everything written for them, by the store's own record of whose each
    /// row is.
    pub async fn person_content_seqs(&self, person: &str) -> Result<Vec<i64>> {
        if !self.store {
            return Ok(vec![]);
        }
        let rows = self.readers.get().await?.query("SELECT seq FROM public.content WHERE person = $1 ORDER BY seq", &[&person]).await.context("find a person's content")?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The latest events of these kinds, newest first, without their
    /// content: each one's seq, time, actor and what the log keeps of it.
    pub async fn of_kinds(&self, kinds: &[&str], limit: i64) -> Result<Vec<(i64, chrono::DateTime<chrono::Utc>, String, Value)>> {
        let kinds: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
        let rows = self
            .readers
            .get()
            .await?
            .query("SELECT seq, at, actor, body FROM public.events WHERE kind = ANY($1) ORDER BY seq DESC LIMIT $2", &[&kinds, &limit])
            .await
            .context("read events by kind")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
    }

    /// One bundle path's writes and removals up to `seq`, oldest first.
    pub async fn bundle_history(&self, path: &str, upto: i64) -> Result<Vec<(i64, Value)>> {
        let sql = "SELECT seq, body FROM public.events WHERE kind IN ('bundle_written', 'bundle_removed') AND body->>'path' = $1 AND seq <= $2 ORDER BY seq";
        let rows = self.readers.get().await?.query(sql, &[&path, &upto]).await.context("read a bundle path's history")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// One bundle path's latest write or removal strictly before `at`:
    /// its event, kind and body. None when it never changed before then.
    pub async fn bundle_at(&self, path: &str, at: DateTime<Utc>) -> Result<Option<(i64, String, Value)>> {
        let sql = "SELECT seq, kind, body FROM public.events WHERE kind IN ('bundle_written', 'bundle_removed') AND body->>'path' = $1 AND at < $2 ORDER BY seq DESC LIMIT 1";
        let rows = self.readers.get().await?.query(sql, &[&path, &at]).await.context("read a bundle path as of a time")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))).next())
    }

    /// Delete the content of these events.
    pub async fn delete_content(&self, seqs: &[i64]) -> Result<u64> {
        if !self.store {
            return Ok(0);
        }
        self.client().await?.execute("DELETE FROM public.content WHERE seq = ANY($1)", &[&seqs]).await.context("delete content")
    }

    /// Every removal so far: the seq of its `removed` event and the events
    /// whose content it deleted, oldest first.
    pub async fn removals(&self) -> Result<Vec<(i64, Vec<i64>)>> {
        let rows = self.readers.get().await?.query("SELECT seq, body->'seqs' FROM public.events WHERE kind = 'removed' ORDER BY seq", &[]).await?;
        Ok(rows.iter().map(|r| (r.get::<_, i64>(0), serde_json::from_value(r.get::<_, Value>(1)).unwrap_or_default())).collect())
    }

    /// Every event a removal named, by its number and its time: the two
    /// together name it in this log's history only, not an event a restore
    /// of an older backup later gave the same number (#195).
    pub async fn removed_events(&self) -> Result<Vec<(i64, DateTime<Utc>)>> {
        let sql = "SELECT seq, at FROM public.events WHERE seq IN (SELECT jsonb_array_elements_text(body->'seqs')::bigint FROM public.events WHERE kind = 'removed') ORDER BY seq";
        let rows = self.readers.get().await?.query(sql, &[]).await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// Finish removals that stopped after their record: delete the content
    /// of every event a `removed` event names. Run at start.
    pub async fn finish_removals(&self) -> Result<u64> {
        if !self.store {
            return Ok(0);
        }
        let sql = "DELETE FROM public.content WHERE seq IN (SELECT jsonb_array_elements_text(body->'seqs')::bigint FROM public.events WHERE kind = 'removed')";
        self.client().await?.execute(sql, &[]).await.context("finish removals")
    }

    /// One run's events in order: the developer view's full trace (V4).
    pub async fn run_events(&self, run: &str) -> Result<Vec<Event>> {
        let sql = format!("SELECT e.seq, e.at, e.actor, e.body, c.body FROM {} WHERE e.run_id = $1 ORDER BY e.seq", self.rows());
        let rows = self.readers.get().await?.query(&sql, &[&run]).await?;
        rows.into_iter()
            .map(|r| {
                let ev: Ev = serde_json::from_value(join(r.get(3), r.get(4)))?;
                Ok(Event { seq: r.get(0), at: r.get(1), actor: r.get(2), ev })
            })
            .collect()
    }

    /// Every row as stored, with its content and salt, for a backup.
    pub async fn dump(&self) -> Result<Vec<Value>> {
        let sql = format!("SELECT e.seq, e.at, e.kind, e.actor, e.run_id, e.body, c.salt, c.body FROM {} ORDER BY e.seq", self.rows());
        let rows = self.readers.get().await?.query(&sql, &[]).await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let at: DateTime<Utc> = r.get(1);
                let salt: Option<Vec<u8>> = r.get(6);
                json!({ "seq": r.get::<_, i64>(0), "at": at.to_rfc3339(), "kind": r.get::<_, String>(2), "actor": r.get::<_, String>(3), "run_id": r.get::<_, Option<String>>(4), "body": r.get::<_, Value>(5), "salt": salt.map(hex::encode), "content": r.get::<_, Option<Value>>(7) })
            })
            .collect())
    }

    pub async fn load(&self) -> Result<Vec<Event>> {
        let sql = format!("SELECT e.seq, e.at, e.actor, e.body, c.body FROM {} ORDER BY e.seq", self.rows());
        let rows = self.client().await?.query(&sql, &[]).await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let body = join(r.get(3), r.get(4));
            let seq: i64 = r.get(0);
            self.saw(seq);
            let Some(ev) = parse_event(&body).with_context(|| format!("event {seq} does not parse"))? else {
                tracing::warn!("event {seq} is a kind this release does not know ({}); skipped", body["kind"]);
                continue;
            };
            out.push(Event { seq, at: r.get(1), actor: r.get(2), ev });
        }
        Ok(out)
    }
}

/// An event from the log: `None` when its kind is one this release does not
/// know (written by a later release it was rolled back from, so nothing it
/// acts on). Any other failure is an error, an unknown value inside a known
/// kind included: serde words both "unknown variant", and skipping the
/// second would drop part of a run's history.
pub fn parse_event(body: &Value) -> Result<Option<Ev>> {
    let current = crate::events::current_lane_dispatched(body).or_else(|| crate::staging::current_staged(body));
    let body = current.as_ref().unwrap_or(body);
    match serde_json::from_value::<Ev>(body.clone()) {
        Ok(ev) => Ok(Some(ev)),
        Err(e) => {
            let kind_unknown = serde_json::from_value::<Ev>(serde_json::json!({ "kind": body["kind"] })).err().is_some_and(|k| k.to_string().starts_with("unknown variant"));
            if kind_unknown {
                Ok(None)
            } else {
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn only_an_unknown_kind_is_skipped() {
        let known = serde_json::to_value(Ev::MemoryForgotten { id: "m1".into() }).unwrap();
        assert!(matches!(parse_event(&known), Ok(Some(_))));
        assert!(matches!(parse_event(&serde_json::json!({ "kind": "from_a_later_release", "x": 1 })), Ok(None)));
        // A known kind holding a value this release does not know (a block
        // type from a later release) stops the boot; serde words it the same.
        let nested = serde_json::json!({ "kind": "memory_pass", "run": "r", "model": "m", "blocks": [{ "type": "thinking_v9", "text": "x" }], "usage": {} });
        let err = serde_json::from_value::<Ev>(nested.clone()).unwrap_err().to_string();
        assert!(err.starts_with("unknown variant"), "{err}");
        assert!(parse_event(&nested).is_err());
    }
}

#[cfg(test)]
#[path = "log_content_tests.rs"]
mod tests;
