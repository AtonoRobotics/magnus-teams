//! The knowledge index's one owner. The `public.knowledge` table holds each
//! entry's full text for search, so it follows the log: every event that
//! adds, changes, trashes, restores or removes an entry, and every erasure,
//! marks what it touched as out of date (`Kernel::emit_all` hands each
//! write here), and the marks are worked off at once, then retried every
//! `RETRY_EVERY` until they hold. A trashed or removed entry's row goes
//! before its content is deleted, as the `Removed` that names it is written
//! first; a search never returns an entry the state no longer has, even
//! while its row waits for a retry (`Kernel::knowledge_for`).

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::knowledge::{shown, text_of};
use crate::log::Event;
use crate::state::KnowledgeEntry;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_postgres::Client;

/// How often marks the index could not take are tried again.
pub const RETRY_EVERY: Duration = Duration::from_secs(30);
/// Entries a search returns.
const HITS: i64 = 8;

/// What the index is behind on.
#[derive(Default, Debug)]
struct Marks {
    /// Entries to index as the state has them, or take out.
    ids: BTreeSet<String>,
    /// Rows of entries the state no longer has (an erasure took them).
    strays: bool,
    /// Everything: at start, or after a full catch-up failed.
    everything: bool,
    /// Why the last try failed, while anything is still marked.
    failure: Option<String>,
}

impl Marks {
    fn is_empty(&self) -> bool {
        self.ids.is_empty() && !self.strays && !self.everything
    }
}

/// The index's own connection to Postgres, opened when first used and
/// again after it was lost, and what it is behind on.
pub struct Index {
    url: String,
    reader: String,
    client: tokio::sync::Mutex<Option<Client>>,
    marks: Mutex<Marks>,
    /// One catch-up at a time: two would race an entry's row.
    catching_up: tokio::sync::Mutex<()>,
}

impl Index {
    /// An index behind on everything, until the first catch-up.
    pub fn new(url: &str) -> Index {
        let role = url::Url::parse(url).map(|u| u.username().to_string()).unwrap_or_default();
        let marks = Mutex::new(Marks { everything: true, ..Default::default() });
        Index { url: url.into(), reader: format!("{role}_reader"), client: tokio::sync::Mutex::new(None), marks, catching_up: tokio::sync::Mutex::new(()) }
    }

    fn marks(&self) -> std::sync::MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Mark what these events touched. True when any did.
    fn follow(&self, es: &[Event]) -> bool {
        let mut m = self.marks();
        let mut any = false;
        for e in es {
            match &e.ev {
                Ev::KnowledgeAdded { id, .. } | Ev::KnowledgeShared { id, .. } | Ev::KnowledgeEndorsed { id, .. } => {
                    m.ids.insert(id.clone());
                }
                Ev::Trashed { item, .. } | Ev::Restored { item } | Ev::Removed { item, .. } if is_entry(item) => {
                    m.ids.insert(item.clone());
                }
                Ev::PersonErased { .. } => m.strays = true,
                _ => continue,
            }
            any = true;
        }
        any
    }

    /// Why an entry is not in the index as the state has it, while a try
    /// to put it there has not held.
    pub fn behind_on(&self, id: &str) -> Option<String> {
        let m = self.marks();
        m.ids.contains(id).then(|| m.failure.clone().unwrap_or_else(|| "it is waiting to be indexed".into()))
    }

    fn behind(&self) -> bool {
        !self.marks().is_empty()
    }

    async fn client(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Client>>> {
        let mut g = self.client.lock().await;
        if g.as_ref().is_none_or(|c| c.is_closed()) {
            *g = Some(crate::log::connect(&self.url).await?);
        }
        Ok(g)
    }

    /// What an entry says (its title, text, source, evidence and value),
    /// read from the content store; None once it was deleted. The kernel
    /// reads it to index it, whoever it belongs to; people read it from the
    /// index, as its readers.
    pub(crate) async fn content(&self, e: &KnowledgeEntry) -> Result<Option<Value>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("index connection")?;
        let row = c.query_opt("SELECT body FROM public.content WHERE seq = $1", &[&e.seq]).await.context("read the entry's content")?;
        Ok(row.map(|r| r.get::<_, Value>(0)))
    }

    /// Index an entry as it now stands, with what it says. `embedding` is
    /// its model and vector.
    async fn put(&self, e: &KnowledgeEntry, says: &Value, embedding: Option<(String, String)>) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("index connection")?;
        let (model, vector) = embedding.unzip();
        c.execute(
            "INSERT INTO public.knowledge (id, readers, integrity, entry, version, model, embedding, words)
             VALUES ($1, $2, $3, $4, $5, $6, $7::text::vector, to_tsvector('simple', $8))
             ON CONFLICT (id) DO UPDATE SET readers = EXCLUDED.readers, integrity = EXCLUDED.integrity, entry = EXCLUDED.entry,
                 version = EXCLUDED.version, model = EXCLUDED.model, embedding = EXCLUDED.embedding, words = EXCLUDED.words
             WHERE knowledge.version <= EXCLUDED.version",
            &[&e.id, &json!(e.readers), &e.integrity, &shown(e, says), &e.version, &model, &vector, &format!("{} {}", e.title, text_of(says))],
        )
        .await
        .context("index the entry (has `magnus migrate` run since this release was installed?)")?;
        Ok(())
    }

    /// Take an entry out of the index: trashed or removed, no one finds it.
    async fn drop_entry(&self, id: &str) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("index connection")?;
        c.execute("DELETE FROM public.knowledge WHERE id = $1", &[&id]).await.context("take the entry out of the index")?;
        Ok(())
    }

    /// What each indexed entry's version and embedding model are.
    async fn versions(&self) -> Result<std::collections::HashMap<String, (i64, Option<String>)>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("index connection")?;
        let rows = c.query("SELECT id, version, model FROM public.knowledge", &[]).await?;
        Ok(rows.iter().map(|r| (r.get::<_, String>(0), (r.get::<_, i64>(1), r.get::<_, Option<String>>(2)))).collect())
    }

    /// The entries these principals may read that best match the query (by
    /// meaning with an embedding, and by words those not yet embedded; by
    /// words without), or the latest with no query. Postgres filters them as the reader role.
    pub(crate) async fn search(&self, principals: &[String], query: &str, embedding: Option<(String, String)>) -> Result<Vec<Value>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("index connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader)).await?;
        tx.execute("SELECT set_config('magnus.principals', $1, true)", &[&serde_json::to_string(principals)?]).await?;
        let words: Vec<String> = query.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(String::from).collect();
        let rows = match embedding {
            Some((model, v)) => {
                // An entry not embedded with this model (saved while the
                // server was down, until the next sync embeds it) is found
                // by its words, first, as it matched them.
                let mut rows = match words.is_empty() {
                    true => vec![],
                    false => tx
                        .query(
                            "SELECT entry FROM public.knowledge WHERE model IS DISTINCT FROM $1 AND words @@ to_tsquery('simple', $2) ORDER BY ts_rank(words, to_tsquery('simple', $2)) DESC, version DESC LIMIT $3",
                            &[&model, &words.join(" | "), &HITS],
                        )
                        .await?,
                };
                let room = HITS - rows.len() as i64;
                rows.extend(tx.query("SELECT entry FROM public.knowledge WHERE model = $1 ORDER BY embedding <=> $2::text::vector LIMIT $3", &[&model, &v, &room]).await?);
                rows
            }
            None if query.trim().is_empty() => tx.query("SELECT entry FROM public.knowledge ORDER BY version DESC LIMIT 50", &[]).await?,
            None if words.is_empty() => vec![],
            None => {
                let q = words.join(" | ");
                tx.query(
                    "SELECT entry FROM public.knowledge WHERE words @@ to_tsquery('simple', $1) ORDER BY ts_rank(words, to_tsquery('simple', $1)) DESC, version DESC LIMIT $2",
                    &[&q, &HITS],
                )
                .await?
            }
        };
        tx.commit().await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }
}

/// A knowledge entry's id, as the trash names it.
fn is_entry(item: &str) -> bool {
    item.starts_with("kb_")
}

impl Kernel {
    /// The index follows a write: what it touched is marked and, with the
    /// marks before it, worked off now. What fails stays marked for the
    /// retry; the write itself stands either way.
    pub(crate) async fn follow_index(&self, es: &[Event]) {
        if self.knowledge.follow(es) {
            if let Err(e) = self.catch_up_index(false).await {
                tracing::warn!("knowledge index: {e:#}; tried again within {}s", RETRY_EVERY.as_secs());
            }
        }
    }

    /// Work off the index's marks: the entries marked, the rows of entries
    /// gone, and with `full` (the retry) everything, while the start's
    /// catch-up or an earlier full one has not held. What fails is marked
    /// again, with why.
    pub(crate) async fn catch_up_index(&self, full: bool) -> Result<()> {
        let _one = self.knowledge.catching_up.lock().await;
        let todo = {
            let mut m = self.knowledge.marks();
            let everything = full && m.everything;
            m.everything &= !everything;
            Marks { ids: std::mem::take(&mut m.ids), strays: std::mem::take(&mut m.strays), everything, failure: None }
        };
        if todo.is_empty() {
            return Ok(());
        }
        let mut left = Marks::default();
        let mut failed: Option<anyhow::Error> = None;
        if todo.everything {
            if let Err(e) = self.sync_knowledge().await {
                (left.everything, failed) = (true, Some(e));
            }
        } else if todo.strays {
            if let Err(e) = self.drop_strays().await {
                (left.strays, failed) = (true, Some(e));
            }
        }
        // A full catch-up that held indexed every entry as it is now.
        let synced = todo.everything && failed.is_none();
        for id in todo.ids.into_iter().filter(|_| !synced) {
            if let Err(e) = self.index_id(&id).await {
                left.ids.insert(id);
                failed = Some(e);
            }
        }
        let mut m = self.knowledge.marks();
        m.ids.extend(left.ids);
        m.strays |= left.strays;
        m.everything |= left.everything;
        m.failure = match &failed {
            Some(e) => Some(format!("{e:#}")),
            None if m.is_empty() => None,
            None => m.failure.take(),
        };
        failed.map_or(Ok(()), Err)
    }

    /// Bring the whole index up to date with the log: entries it lacks, or
    /// holds at an older version or another embedding model, and rows of
    /// entries gone. At start (a restore, a missed write, a new model).
    async fn sync_knowledge(&self) -> Result<()> {
        let have = self.knowledge.versions().await?;
        let want = self.cfg.knowledge.embeddings.as_ref().map(|e| e.model.clone());
        let entries: Vec<KnowledgeEntry> = self.read(|s| s.library.knowledge().values().cloned().collect());
        // Entries in the trash or removed are no longer in the state.
        for id in have.keys().filter(|id| !entries.iter().any(|e| &e.id == *id)) {
            self.knowledge.drop_entry(id).await?;
        }
        for e in entries {
            if have.get(&e.id).is_some_and(|(v, m)| *v == e.version && *m == want) {
                continue;
            }
            self.index_entry(&e).await?;
        }
        Ok(())
    }

    /// Index one entry with what the content store says it says; without
    /// its embedding (server down) it is still found by its words, and
    /// embedded at the next sync. An entry whose content was deleted is
    /// taken out.
    async fn index_entry(&self, e: &KnowledgeEntry) -> Result<()> {
        let Some(says) = self.knowledge.content(e).await? else { return self.knowledge.drop_entry(&e.id).await };
        let embedding = match self.embed(&format!("{}\n{}", e.title, text_of(&says))).await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!("embed {}: {err:#}", e.id);
                None
            }
        };
        self.knowledge.put(e, &says, embedding).await
    }

    /// Take out the rows of entries the state no longer has.
    async fn drop_strays(&self) -> Result<()> {
        let have = self.knowledge.versions().await?;
        let gone: Vec<String> = self.read(|s| have.into_keys().filter(|id| !s.library.knowledge().contains_key(id)).collect());
        for id in gone {
            self.knowledge.drop_entry(&id).await?;
        }
        Ok(())
    }

    /// Index an entry as the state now has it, or take it out if it has none.
    async fn index_id(&self, id: &str) -> Result<()> {
        if !self.read(|s| s.library.knowledge().contains_key(id)) {
            return self.knowledge.drop_entry(id).await;
        }
        match self.read(|s| s.library.knowledge().get(id).cloned()) {
            Some(e) => self.index_entry(&e).await,
            None => Ok(()),
        }
    }
}

/// Retry what the index is behind on, from the start and every
/// `RETRY_EVERY`, for as long as the kernel runs.
pub fn schedule(k: &Arc<Kernel>) {
    let k = Arc::downgrade(k);
    tokio::spawn(async move {
        loop {
            match k.upgrade() {
                Some(k) if k.knowledge.behind() => {
                    if let Err(e) = k.catch_up_index(true).await {
                        tracing::warn!("knowledge index: {e:#}");
                    }
                }
                Some(_) => {}
                None => return,
            }
            tokio::time::sleep(RETRY_EVERY).await;
        }
    });
}

#[cfg(test)]
#[path = "knowledge_index_tests.rs"]
pub(crate) mod tests;
