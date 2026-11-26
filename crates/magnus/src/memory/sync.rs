//! Keeps each person's sources in the unified store equal to their current
//! legacy memory, off the request path.
//!
//! `Kernel::emit_all` only marks who a write touched (`MemorySync::follow`).
//! A background pass then makes the store match the state for each marked
//! person: it copies current memories that are missing and hard-deletes
//! sources that are no longer current (forgotten, archived, superseded,
//! erased). The pass is bounded by `SYNC_PASS_TIMEOUT_MS`. Marks it could not
//! clear are kept and tried again every `SYNC_RETRY_EVERY_SECS`, and
//! everything is marked at start. Deletion runs whenever the tables exist,
//! whatever the mode; copying runs only for people not on `MemoryMode::Legacy`.
//!
//! The unified store serves a person (`MemorySync::is_synced`) only once a
//! pass has finished for them, with nothing marked since, and with no
//! forgotten memory waiting to be deleted. Until then they get legacy recall.

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::log::Event;
use crate::memory::ingest::ingest_source;
use crate::memory::store::MemoryStore;
use crate::memory::{SourceInput, TaintLabel};
use crate::memory_index::MemoryMode;
use crate::recall::Remembered;
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often marks a pass could not clear are tried again.
pub const SYNC_RETRY_EVERY_SECS: Duration = Duration::from_secs(30);
/// Longest one sync pass may run before it is abandoned (its marks are kept).
pub const SYNC_PASS_TIMEOUT_MS: Duration = Duration::from_millis(60_000);

/// What the unified store is behind on.
#[derive(Default, Debug, Clone)]
struct Marks {
    /// People whose sources must be made to match their current memory.
    people: BTreeSet<String>,
    /// Memory ids forgotten since the last pass. Forgetting names no person,
    /// so every source with the id is deleted.
    forgotten: BTreeSet<String>,
    /// Everyone: at start, or after a full pass failed.
    everything: bool,
}

impl Marks {
    fn is_empty(&self) -> bool {
        self.people.is_empty() && self.forgotten.is_empty() && !self.everything
    }

    fn merge(&mut self, other: Marks) {
        self.people.extend(other.people);
        self.forgotten.extend(other.forgotten);
        self.everything |= other.everything;
    }
}

/// The sync's marks, who is in step, and the wake-up for its background task.
pub struct MemorySync {
    marks: Mutex<Marks>,
    synced: Mutex<HashSet<String>>,
    wake: tokio::sync::Notify,
    /// One pass at a time: two would race a person's sources.
    running: tokio::sync::Mutex<()>,
}

impl Default for MemorySync {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySync {
    /// Behind on everyone, until the first pass.
    pub fn new() -> Self {
        Self {
            marks: Mutex::new(Marks { everything: true, ..Marks::default() }),
            synced: Mutex::new(HashSet::new()),
            wake: tokio::sync::Notify::new(),
            running: tokio::sync::Mutex::new(()),
        }
    }

    /// Mark what committed events changed and wake the background pass. Takes
    /// no database work, so a save never waits on the unified store.
    pub fn follow(&self, es: &[Event]) {
        let mut touched = Marks::default();
        for e in es {
            match &e.ev {
                Ev::Memory { person, .. } | Ev::PersonErased { person } => {
                    touched.people.insert(person.clone());
                }
                Ev::MemorySuperseded(p) => {
                    touched.people.insert(p.person.clone());
                }
                Ev::MemoryArchived(p) => {
                    touched.people.insert(p.person.clone());
                }
                Ev::MemoryForgotten { id } => {
                    touched.forgotten.insert(id.clone());
                }
                _ => {}
            }
        }
        if touched.is_empty() {
            return;
        }
        {
            let mut synced = self.synced.lock().unwrap_or_else(PoisonError::into_inner);
            for p in &touched.people {
                synced.remove(p);
            }
        }
        self.marks.lock().unwrap_or_else(PoisonError::into_inner).merge(touched);
        self.wake.notify_one();
    }

    /// True when the unified store holds exactly `person`'s current memory.
    pub fn is_synced(&self, person: &str) -> bool {
        {
            let m = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
            if m.everything || !m.forgotten.is_empty() || m.people.contains(person) {
                return false;
            }
        }
        self.synced.lock().unwrap_or_else(PoisonError::into_inner).contains(person)
    }

    /// Mark everyone, as at start (after the tables were created, or to rebuild).
    pub fn mark_everything(&self) {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner).everything = true;
        self.wake.notify_one();
    }

    /// True while anything is marked.
    pub fn behind(&self) -> bool {
        !self.marks.lock().unwrap_or_else(PoisonError::into_inner).is_empty()
    }

    fn take(&self) -> Marks {
        std::mem::take(&mut *self.marks.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn put_back(&self, m: Marks) {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner).merge(m);
    }

    /// Record `person` as in step, unless a write marked them again meanwhile.
    fn settle(&self, person: &str) {
        let m = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        if m.everything || m.people.contains(person) {
            return;
        }
        self.synced.lock().unwrap_or_else(PoisonError::into_inner).insert(person.to_string());
    }
}

#[cfg(test)]
impl MemorySync {
    pub(crate) fn take_for_test(&self) {
        self.take();
    }

    pub(crate) fn settle_for_test(&self, person: &str) {
        self.settle(person);
    }
}

/// Run a pass whenever a write wakes the sync, and every
/// `SYNC_RETRY_EVERY_SECS` while it is behind, for as long as the kernel runs.
pub fn schedule(k: &Arc<Kernel>) {
    let weak = Arc::downgrade(k);
    tokio::spawn(async move {
        loop {
            let Some(k) = weak.upgrade() else { return };
            if k.memory_sync.behind() {
                if let Err(e) = k.sync_memory().await {
                    tracing::warn!("memory sync: {e:#}; tried again within {}s", SYNC_RETRY_EVERY_SECS.as_secs());
                }
            }
            let wake = async { k.memory_sync.wake.notified().await };
            tokio::select! {
                _ = wake => {}
                _ = tokio::time::sleep(SYNC_RETRY_EVERY_SECS) => {}
            }
        }
    });
}

fn format_memory_text(fact: &str, topic: Option<&str>) -> String {
    match topic.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("[{t}] {fact}"),
        None => fact.to_string(),
    }
}

impl Kernel {
    /// One sync pass over what is marked, bounded by `SYNC_PASS_TIMEOUT_MS`.
    /// What it could not finish stays marked.
    pub async fn sync_memory(&self) -> Result<()> {
        let _one = self.memory_sync.running.lock().await;
        let todo = self.memory_sync.take();
        if todo.is_empty() {
            return Ok(());
        }
        let outcome = tokio::time::timeout(SYNC_PASS_TIMEOUT_MS, self.sync_marked(&todo)).await;
        match outcome {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                self.memory_sync.put_back(todo);
                Err(e)
            }
            Err(_) => {
                self.memory_sync.put_back(todo);
                anyhow::bail!("memory sync pass ran past {} ms", SYNC_PASS_TIMEOUT_MS.as_millis())
            }
        }
    }

    async fn sync_marked(&self, todo: &Marks) -> Result<()> {
        let cfg = &self.cfg.memory;
        let mut store = MemoryStore::connect(&self.cfg.database.url, cfg).await?;
        let present: bool =
            store.client().query_one("SELECT to_regclass('public.memory_sources') IS NOT NULL", &[]).await.context("check whether the unified memory tables exist")?.get(0);
        if !present {
            // Nothing to keep in step; nobody is served from the store either.
            return Ok(());
        }
        for id in &todo.forgotten {
            store.hard_delete_source_all_revs(&cfg.tenant_id, id).await.with_context(|| format!("delete forgotten memory {id}"))?;
        }
        let mut people = todo.people.clone();
        if todo.everything {
            people.extend(self.read(|s| s.library.memory().iter().map(|m| m.person.clone()).filter(|p| cfg.mode_for(p) != MemoryMode::Legacy).collect::<Vec<_>>()));
            let owners =
                store.client().query("SELECT DISTINCT owner_id FROM public.memory_sources WHERE tenant_id = $1", &[&cfg.tenant_id]).await.context("list unified memory owners")?;
            people.extend(owners.iter().map(|r| r.get::<_, String>(0)));
        }
        for person in &people {
            self.sync_person(&mut store, person).await.with_context(|| format!("sync memory for {person}"))?;
            self.memory_sync.settle(person);
        }
        Ok(())
    }

    /// Make `person`'s sources equal their current memory: not archived, not
    /// superseded. Sources no longer current are deleted in every mode;
    /// missing ones are copied only when the person is not on Legacy.
    async fn sync_person(&self, store: &mut MemoryStore, person: &str) -> Result<()> {
        let tenant = self.cfg.memory.tenant_id.clone();
        let current: HashSet<String> = self.read(|s| {
            s.library
                .memory()
                .iter()
                .filter(|m| m.person == person && !s.memory_archived.contains(&m.id) && !s.memory_superseded.contains_key(&m.id))
                .map(|m| m.id.clone())
                .collect()
        });
        let have: HashSet<String> = store
            .client()
            .query("SELECT DISTINCT source_id FROM public.memory_sources WHERE tenant_id = $1 AND owner_id = $2", &[&tenant, &person])
            .await
            .context("list the person's unified sources")?
            .iter()
            .map(|r| r.get(0))
            .collect();
        for stale in have.difference(&current) {
            store.hard_delete_source_all_revs(&tenant, stale).await?;
        }
        if self.cfg.memory.mode_for(person) == MemoryMode::Legacy {
            return Ok(());
        }
        let missing: HashSet<&String> = current.difference(&have).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let to_copy: Vec<Remembered> = self.remembered(person, |m| missing.contains(&m.id)).await?;
        for m in &to_copy {
            let input = SourceInput {
                tenant_id: tenant.clone(),
                source_id: m.entry.id.clone(),
                rev: m.entry.seq,
                owner_id: person.to_string(),
                readers: vec![vec![format!("user:{person}")]],
                label: TaintLabel { untrusted: m.entry.label.untrusted, private: m.entry.label.private },
                text: format_memory_text(&m.fact, m.topic.as_deref()),
                occurred_at: m.entry.at,
            };
            ingest_source(store, &input).await?;
        }
        Ok(())
    }
}
