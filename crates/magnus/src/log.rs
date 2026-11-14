//! The event log: one append-only Postgres table, the source of truth for
//! what happened, and beside it the content store, `content`, holding what
//! people and sources wrote (`events::CONTENT`), where it can be deleted.
//!
//! `migrate` runs with an administrator URL and creates the tables and the
//! kernel's role (and the knowledge base's index, `knowledge::migrate_sql`).
//! On `events` the role may INSERT and SELECT and nothing else, so the
//! database itself refuses rewrites of history. Everything else the kernel
//! knows is a projection of the log and its content, rebuilt on boot.

use crate::events::Ev;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, NoTls};

/// How often startup asks for the log's database again while it waits.
const OPEN_RETRY: std::time::Duration = std::time::Duration::from_millis(250);

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

fn ident(s: &str) -> Result<&str> {
    anyhow::ensure!(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "unsafe SQL identifier {s:?}");
    Ok(s)
}

/// A URL's password as Postgres is to store it: percent-decoded, as the
/// kernel sends it when it connects, and quoted for a SQL string.
fn sql_password(in_url: &str) -> Result<String> {
    let pw = percent_encoding::percent_decode_str(in_url).decode_utf8().context("the database password is not UTF-8")?;
    Ok(pw.replace('\'', "''"))
}

/// Create the log table and the kernel role named in `kernel_url`, and,
/// when the admin console is set up, its root role named in `root_url`.
pub async fn migrate(admin_url: &str, kernel_url: &str, root_url: Option<&str>) -> Result<()> {
    let k = url::Url::parse(kernel_url).context("database.url")?;
    let role = ident(k.username())?.to_string();
    let password = sql_password(k.password().context("database.url has no password")?)?;
    let db = ident(k.path().trim_start_matches('/'))?.to_string();
    let admin = connect(admin_url).await?;
    let exists = admin.query_opt("SELECT 1 FROM pg_roles WHERE rolname = $1", &[&role]).await?.is_some();
    // A role made beforehand keeps whatever it was given, so its attributes
    // are set either way; what it may still do to the log is checked below.
    let verb = if exists { "ALTER" } else { "CREATE" };
    admin.batch_execute(&format!("{verb} ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS")).await?;
    admin
        .batch_execute(&format!(
            "CREATE TABLE IF NOT EXISTS public.events (
                seq    bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                at     timestamptz NOT NULL DEFAULT now(),
                kind   text NOT NULL,
                actor  text NOT NULL,
                run_id text,
                body   jsonb NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_run ON public.events (run_id, seq);
             REVOKE ALL ON public.events FROM PUBLIC;
             REVOKE ALL ON public.events FROM {role};
             GRANT CONNECT ON DATABASE {db} TO {role};
             GRANT USAGE ON SCHEMA public TO {role};
             REVOKE CREATE ON SCHEMA public FROM PUBLIC;
             GRANT SELECT, INSERT ON public.events TO {role};
             CREATE TABLE IF NOT EXISTS public.content (
                seq  bigint PRIMARY KEY REFERENCES public.events (seq),
                salt bytea NOT NULL,
                body jsonb NOT NULL
             );
             REVOKE ALL ON public.content FROM PUBLIC;
             REVOKE ALL ON public.content FROM {role};
             GRANT SELECT, INSERT, DELETE ON public.content TO {role};"
        ))
        .await?;
    move_inline_content(&admin).await.context("move content written into the log to the content store")?;
    admin.batch_execute(&content_walls_sql(&role)).await.context("wall the content store by person")?;
    // The knowledge base's index, filtered for each reader by row security.
    admin.batch_execute(&crate::knowledge::migrate_sql(&role)).await.context("create the knowledge index (pgvector must be installed)")?;
    admin.batch_execute(&crate::computer::migrate_sql(&role)).await.context("create the computers' egress audit table")?;
    admin.batch_execute(&crate::accounts::migrate_sql(&role)).await.context("create the accounts tables")?;
    // Append-only is the database's to enforce: whatever grants or
    // memberships the role holds (pg_write_all_data, owning the table, a
    // role that does), it must not be able to change or delete the log.
    // A membership counts whether or not the role inherits it: a NOINHERIT
    // role holds nothing through it until it switches to it with SET ROLE
    // (#186). So every role it is a member of, directly or through others,
    // itself included, is checked as if the kernel were that role: none may
    // change the log, hold what the kernel was stripped of above, or reach
    // the server's files and programs (Postgres documents those three roles
    // as a way to superuser). Memberships are refused, not revoked, as an
    // inherited one always was: they were given on purpose by someone, and
    // taking them away is theirs to decide.
    let can: Vec<String> = admin
        .query(
            "SELECT rolname || ': ' || array_to_string(rights, ', ') FROM (
                SELECT r.rolname, array_remove(ARRAY[
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'UPDATE') THEN 'UPDATE' END,
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'DELETE') THEN 'DELETE' END,
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'TRUNCATE') THEN 'TRUNCATE' END,
                    CASE WHEN r.rolsuper THEN 'SUPERUSER' END,
                    CASE WHEN r.rolcreatedb THEN 'CREATEDB' END,
                    CASE WHEN r.rolcreaterole THEN 'CREATEROLE' END,
                    CASE WHEN r.rolreplication THEN 'REPLICATION' END,
                    CASE WHEN r.rolbypassrls THEN 'BYPASSRLS' END,
                    CASE WHEN r.rolname IN ('pg_read_server_files', 'pg_write_server_files', 'pg_execute_server_program') THEN 'server files' END
                ], NULL) AS rights
                FROM pg_roles r WHERE pg_has_role($1, r.oid, 'MEMBER')
             ) m WHERE cardinality(rights) > 0 ORDER BY rolname",
            &[&role],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    anyhow::ensure!(
        can.is_empty(),
        "the kernel's role {role} can change the log or act past its grants ({}), through a grant or a role it is a member of, inherited or by SET ROLE; take that away and run migrate again",
        can.join("; ")
    );
    if let Some(root) = root_url {
        migrate_root(&admin, root, &db).await.context("the admin console's database role")?;
    }
    Ok(())
}

/// Move the content of events written whole, before the content store
/// existed, out of the log and into the store, where trash and erasure can
/// delete it, leaving the record with the content's hash as `split` does for
/// every event now. Each event moves in one statement, so a crash leaves it
/// either whole or moved; events already moved, or without content, are left.
async fn move_inline_content(admin: &Client) -> Result<()> {
    // Only kinds that carry content, and only records without the hash a
    // split leaves: a record with it was split when written or moved
    // before, and its content, if gone, was deleted on purpose.
    let kinds: Vec<&str> = crate::events::CONTENT.iter().map(|(k, _)| *k).collect();
    let mut after = 0i64;
    loop {
        let rows = admin
            .query(
                "SELECT e.seq, e.body FROM public.events e
                 WHERE e.seq > $1 AND e.kind = ANY($2) AND NOT (e.body ? 'content')
                 ORDER BY e.seq LIMIT 1000",
                &[&after, &kinds],
            )
            .await?;
        let Some(last) = rows.last() else { return Ok(()) };
        after = last.get(0);
        for r in &rows {
            let (record, Some(content)) = split(r.get(1)) else { continue };
            admin
                .execute(
                    "WITH moved AS (INSERT INTO public.content (seq, salt, body) VALUES ($1, $2, $3) RETURNING seq)
                     UPDATE public.events SET body = $4 WHERE seq = (SELECT seq FROM moved)",
                    &[&r.get::<_, i64>(0), &content.salt, &content.body, &record],
                )
                .await?;
        }
    }
}

/// The admin console's root role (admin console spec, Authority and audit):
/// it reads every person's content, past the walls between people
/// (BYPASSRLS), and the log, which it can add to but never change or
/// delete from. No other process holds it.
async fn migrate_root(admin: &Client, root_url: &str, db: &str) -> Result<()> {
    let r = url::Url::parse(root_url).context("admin.database_url")?;
    let role = ident(r.username())?.to_string();
    let password = sql_password(r.password().context("admin.database_url has no password")?)?;
    let exists = admin.query_opt("SELECT 1 FROM pg_roles WHERE rolname = $1", &[&role]).await?.is_some();
    let verb = if exists { "ALTER" } else { "CREATE" };
    admin.batch_execute(&format!("{verb} ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE BYPASSRLS")).await?;
    admin
        .batch_execute(&format!(
            "GRANT CONNECT ON DATABASE {db} TO {role};
             GRANT USAGE ON SCHEMA public TO {role};
             REVOKE ALL ON public.events, public.content, public.knowledge FROM {role};
             GRANT SELECT, INSERT ON public.events TO {role};
             GRANT SELECT ON public.content, public.knowledge TO {role};"
        ))
        .await?;
    Ok(())
}

/// The walls between people in the content store (owner's ruling 9 and
/// "recall only", 2026-11-01; `docs/proposals/recall-only.md`, step 1):
/// each row records the person it belongs to, and `{role}_person` reads
/// only the rows of the person in `magnus.person`, under a row policy,
/// whatever query it is given. The kernel's own role still reads every
/// row. The policy is not forced on the table's owner, the administrator
/// `migrate` runs as, which fills the column below. Rows written before the column existed are filled from their
/// events, by the same rule as `State::content_owner`.
pub fn content_walls_sql(role: &str) -> String {
    let reader = format!("{role}_person");
    let owners = CONTENT_OWNERS;
    format!(
        "ALTER TABLE public.content ADD COLUMN IF NOT EXISTS person text;
         CREATE INDEX IF NOT EXISTS content_person ON public.content (person);
         DO $$ BEGIN CREATE ROLE {reader} NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;
         DO $$ BEGIN IF NOT pg_has_role('{role}', '{reader}', 'MEMBER') THEN GRANT {reader} TO {role}; END IF; END $$;
         REVOKE ALL ON public.content FROM {reader};
         REVOKE ALL ON public.events FROM {reader};
         GRANT USAGE ON SCHEMA public TO {reader};
         GRANT SELECT ON public.content TO {reader};
         ALTER TABLE public.content ENABLE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS content_kernel ON public.content;
         CREATE POLICY content_kernel ON public.content FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS content_person ON public.content;
         CREATE POLICY content_person ON public.content FOR SELECT TO {reader}
             USING (person IS NOT NULL AND person = NULLIF(current_setting('magnus.person', true), ''));
         {owners}"
    )
}

/// Fill in whose each content row is, for rows that do not say: written
/// before the column existed, or restored from a backup.
pub const CONTENT_OWNERS: &str = "UPDATE public.content c SET person = o.person FROM (
    SELECT e.seq, CASE
        WHEN e.kind IN ('knowledge_added', 'playbook') THEN NULL
        WHEN e.kind = 'bundle_written' THEN e.body->>'owner_user'
        WHEN e.kind = 'item_resolved' THEN (SELECT i.body->>'person' FROM public.events i
            WHERE i.kind = 'item' AND i.body->>'id' = e.body->>'id' ORDER BY i.seq LIMIT 1)
        WHEN e.kind = 'schedule_ended' THEN (SELECT x.body->>'person' FROM public.events x
            WHERE x.kind = 'schedule_set' AND x.body->>'id' = e.body->>'id' ORDER BY x.seq LIMIT 1)
        -- `team::owner`: a message and its edits are the sender's, a
        -- request's are the requester's, a mark is its person's.
        WHEN e.kind = 'team' THEN NULLIF(CASE e.body->>'change'
            WHEN 'message' THEN regexp_replace(w.body->'message'->>'sender', '^agent:', '')
            WHEN 'note' THEN regexp_replace(w.body->'note'->>'by', '^agent:', '')
            WHEN 'run' THEN e.body->>'requester'
            WHEN 'whisper' THEN e.body->>'owner'
            WHEN 'chat' THEN CASE WHEN w.body->'chat'->>'what' = 'edited' THEN (SELECT regexp_replace(mc.body->'message'->>'sender', '^agent:', '')
                FROM public.events m JOIN public.content mc USING (seq) WHERE m.kind = 'team' AND m.body->>'change' = 'message'
                AND m.body->>'workspace' = e.body->>'workspace' AND m.body->>'channel' = w.body->'chat'->>'channel'
                AND mc.body->'message'->>'id' = w.body->'chat'->>'id' ORDER BY m.seq LIMIT 1) ELSE w.body->'chat'->>'person' END
            ELSE e.body->>'person' END, '')
        ELSE COALESCE(e.body->>'person', (SELECT t.body->>'person' FROM public.events t
            WHERE t.kind = 'task_created' AND t.run_id = e.run_id ORDER BY t.seq LIMIT 1))
    END AS person
    FROM public.events e JOIN public.content w USING (seq) WHERE w.person IS NULL
) o WHERE c.seq = o.seq AND o.person IS NOT NULL;";

/// One event's content, apart from its record.
pub struct Content {
    /// Random, kept with the content and deleted with it, so the hash left in
    /// the log cannot be matched by guessing short content once it is gone.
    pub salt: Vec<u8>,
    pub body: Value,
}

/// Split an event's body into the record the log keeps and its content,
/// leaving in the record the content's size and salted SHA-256.
pub fn split(mut body: Value) -> (Value, Option<Content>) {
    let kind = body["kind"].as_str().unwrap_or_default().to_string();
    let mut taken = serde_json::Map::new();
    for f in crate::events::content_fields(&kind) {
        let (outer, inner) = match f.split_once('.') {
            Some((o, i)) => (body.get_mut(o), i),
            None => (Some(&mut body), *f),
        };
        let Some(o) = outer.and_then(Value::as_object_mut) else { continue };
        if let Some(v) = o.get_mut(inner).filter(|v| !v.is_null()).map(Value::take) {
            o.remove(inner);
            taken.insert(f.to_string(), v);
        }
    }
    if taken.is_empty() {
        return (body, None);
    }
    let content = Content { salt: rand::random::<[u8; 32]>().to_vec(), body: Value::Object(taken) };
    // A Value always serializes, and its Display is the same compact JSON as `serde_json::to_vec`.
    let bytes = content.body.to_string().into_bytes();
    body["content"] = json!({ "sha256": hash(&content.salt, &bytes), "bytes": bytes.len() });
    (body, Some(content))
}

fn hash(salt: &[u8], bytes: &[u8]) -> String {
    hex::encode(Sha256::new().chain_update(salt).chain_update(bytes).finalize())
}

/// An event's body as it was written: its record with its content put back.
/// Without its content (deleted), the record alone.
pub fn join(mut body: Value, content: Option<Value>) -> Value {
    let Some(Value::Object(c)) = content else { return body };
    if let Some(o) = body.as_object_mut() {
        o.remove("content");
    }
    for (f, v) in c {
        match f.split_once('.') {
            Some((outer, inner)) => {
                if let Some(o) = body.get_mut(outer).and_then(Value::as_object_mut) {
                    o.insert(inner.into(), v);
                }
            }
            None => body[f] = v,
        }
    }
    body
}

/// Which events hold one item's own content: those of `runs` except the
/// kinds in `except` (what the runs made that is an item of its own); those
/// of `kinds` whose `field` is `id`; each `(kind, field, value)` of
/// `matches` (what belongs to the item but carries no run: its verdict, the
/// answers to its cards); and each `(kind, run, text)` of `containing`, the
/// events of that kind in that run whose content holds the text (where a
/// memory was written).
#[derive(Debug, Default)]
pub struct Holding {
    pub runs: Vec<String>,
    pub except: Vec<&'static str>,
    pub kinds: Vec<&'static str>,
    pub field: &'static str,
    pub id: String,
    pub matches: Vec<(&'static str, &'static str, String)>,
    pub containing: Vec<(&'static str, String, String)>,
}

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
        Ok(Log { url: url.into(), writer, store, owners, client: tokio::sync::RwLock::new(client), last: 0.into() })
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
                    tokio::time::sleep(OPEN_RETRY).await;
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
    pub async fn append_all(&self, evs: &[(&str, Ev)], owners: &[Option<String>]) -> Result<Vec<Event>> {
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
        let row = self.client().await?.query_opt(&sql, &[&run, &id]).await?;
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
        let rows = self.client().await?.query(&sql, &[&seqs]).await.context("read content")?;
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
            .client()
            .await?
            .query(
                "SELECT e.seq FROM public.events e JOIN public.content c USING (seq)
                 WHERE (e.run_id = ANY($1) AND e.kind <> ALL($2)) OR (e.kind = ANY($3) AND e.body->>$4 = $5)
                    OR EXISTS (SELECT 1 FROM unnest($6::text[], $7::text[], $8::text[]) AS m(k, f, v) WHERE e.kind = m.k AND e.body->>m.f = m.v)
                    OR EXISTS (SELECT 1 FROM unnest($9::text[], $10::text[], $11::text[]) AS x(k, r, t) WHERE e.kind = x.k AND e.run_id = x.r AND strpos(c.body::text, x.t) > 0)
                 ORDER BY e.seq",
                &[&h.runs, &except, &kinds, &h.field, &h.id, &mk, &mf, &mv, &ck, &cr, &ct],
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
        let rows = self.client().await?.query("SELECT seq FROM public.content WHERE person = $1 ORDER BY seq", &[&person]).await.context("find a person's content")?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The latest events of these kinds, newest first, without their
    /// content: each one's seq, time, actor and what the log keeps of it.
    pub async fn of_kinds(&self, kinds: &[&str], limit: i64) -> Result<Vec<(i64, chrono::DateTime<chrono::Utc>, String, Value)>> {
        let kinds: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
        let rows = self
            .client()
            .await?
            .query("SELECT seq, at, actor, body FROM public.events WHERE kind = ANY($1) ORDER BY seq DESC LIMIT $2", &[&kinds, &limit])
            .await
            .context("read events by kind")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
    }

    /// One bundle path's writes and removals up to `seq`, oldest first.
    pub async fn bundle_history(&self, path: &str, upto: i64) -> Result<Vec<(i64, Value)>> {
        let sql = "SELECT seq, body FROM public.events WHERE kind IN ('bundle_written', 'bundle_removed') AND body->>'path' = $1 AND seq <= $2 ORDER BY seq";
        let rows = self.client().await?.query(sql, &[&path, &upto]).await.context("read a bundle path's history")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// One bundle path's latest write or removal strictly before `at`:
    /// its event, kind and body. None when it never changed before then.
    pub async fn bundle_at(&self, path: &str, at: DateTime<Utc>) -> Result<Option<(i64, String, Value)>> {
        let sql = "SELECT seq, kind, body FROM public.events WHERE kind IN ('bundle_written', 'bundle_removed') AND body->>'path' = $1 AND at < $2 ORDER BY seq DESC LIMIT 1";
        let rows = self.client().await?.query(sql, &[&path, &at]).await.context("read a bundle path as of a time")?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))).next())
    }

    /// Delete the content of these events.
    pub async fn delete_content(&self, seqs: &[i64]) -> Result<u64> {
        if !self.store {
            return Ok(0);
        }
        Ok(self.client().await?.execute("DELETE FROM public.content WHERE seq = ANY($1)", &[&seqs]).await.context("delete content")?)
    }

    /// Every removal so far: the seq of its `removed` event and the events
    /// whose content it deleted, oldest first.
    pub async fn removals(&self) -> Result<Vec<(i64, Vec<i64>)>> {
        let rows = self.client().await?.query("SELECT seq, body->'seqs' FROM public.events WHERE kind = 'removed' ORDER BY seq", &[]).await?;
        Ok(rows.iter().map(|r| (r.get::<_, i64>(0), serde_json::from_value(r.get::<_, Value>(1)).unwrap_or_default())).collect())
    }

    /// Every event a removal named, by its number and its time: the two
    /// together name it in this log's history only, not an event a restore
    /// of an older backup later gave the same number (#195).
    pub async fn removed_events(&self) -> Result<Vec<(i64, DateTime<Utc>)>> {
        let sql = "SELECT seq, at FROM public.events WHERE seq IN (SELECT jsonb_array_elements_text(body->'seqs')::bigint FROM public.events WHERE kind = 'removed') ORDER BY seq";
        let rows = self.client().await?.query(sql, &[]).await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// Finish removals that stopped after their record: delete the content
    /// of every event a `removed` event names. Run at start.
    pub async fn finish_removals(&self) -> Result<u64> {
        if !self.store {
            return Ok(0);
        }
        let sql = "DELETE FROM public.content WHERE seq IN (SELECT jsonb_array_elements_text(body->'seqs')::bigint FROM public.events WHERE kind = 'removed')";
        Ok(self.client().await?.execute(sql, &[]).await.context("finish removals")?)
    }

    /// One run's events in order: the developer view's full trace (V4).
    pub async fn run_events(&self, run: &str) -> Result<Vec<Event>> {
        let sql = format!("SELECT e.seq, e.at, e.actor, e.body, c.body FROM {} WHERE e.run_id = $1 ORDER BY e.seq", self.rows());
        let rows = self.client().await?.query(&sql, &[&run]).await?;
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
        let rows = self.client().await?.query(&sql, &[]).await?;
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
