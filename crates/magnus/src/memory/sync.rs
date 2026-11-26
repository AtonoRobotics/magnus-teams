//! Keeps each person's sources in the unified store equal to their current
//! legacy memory, off the request path.
//!
//! `Kernel::emit_all` only marks what a write touched (`MemorySync::follow`).
//! A background pass then makes the store match the state for each marked
//! person: it copies current memories that are missing and hard-deletes
//! sources that are no longer current (forgotten, archived, superseded,
//! trashed, erased). A pass is bounded by `SYNC_PASS_TIMEOUT_SECS`. What it
//! could not finish stays marked and is tried again every
//! `SYNC_RETRY_EVERY_SECS`; one person failing does not hold up the rest.
//! Everyone is marked at start. Deletion runs whenever the tables exist,
//! whatever the mode; copying runs only for people not on `MemoryMode::Legacy`.
//!
//! The unified store serves a person (`MemorySync::is_synced`) only once a
//! pass has committed for them, with nothing marked since, and with no
//! forgotten memory waiting to be deleted, whether marked or in a pass still
//! running. Until then they get legacy recall.

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::log::Event;
use crate::memory::erasure::{hold_owner_for_sync, release_owner_for_sync};
use crate::memory::ingest::ingest_source;
use crate::memory::store::MemoryStore;
use crate::memory::{SourceInput, TaintLabel};
use crate::memory_index::MemoryMode;
use crate::recall::Remembered;
use anyhow::Result;
use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often marks a pass could not clear are tried again.
pub const SYNC_RETRY_EVERY_SECS: Duration = Duration::from_secs(30);
/// Longest one sync pass may run before it is abandoned (its marks are kept).
pub const SYNC_PASS_TIMEOUT_SECS: Duration = Duration::from_secs(60);

/// What the unified store is behind on.
#[derive(Default, Debug, Clone)]
struct Marks {
    /// People whose sources must be made to match their current memory.
    people: BTreeSet<String>,
    /// Memory ids forgotten since the last pass. Forgetting names no person,
    /// so every source with the id is deleted.
    forgotten: BTreeSet<String>,
    /// Everyone: at start, or after the list of everyone could not be made.
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

    /// Whether these marks keep `person` from being served from the store.
    fn hold_back(&self, person: &str) -> bool {
        self.everything || !self.forgotten.is_empty() || self.people.contains(person)
    }
}

/// The sync's marks, who is in step, and the wake-up for its background task.
pub struct MemorySync {
    marks: Mutex<Marks>,
    /// What the pass now running took, until it has committed or given up.
    in_flight: Mutex<Marks>,
    synced: Mutex<HashSet<String>>,
    /// Why the last pass failed, while it has not since succeeded.
    failure: Mutex<Option<String>>,
    wake: Arc<tokio::sync::Notify>,
    /// One pass at a time: two would race a person's sources.
    running: tokio::sync::Mutex<()>,
}

impl Default for MemorySync {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl MemorySync {
    /// Behind on everyone, until the first pass.
    pub fn new() -> Self {
        Self {
            marks: Mutex::new(Marks { everything: true, ..Marks::default() }),
            in_flight: Mutex::new(Marks::default()),
            synced: Mutex::new(HashSet::new()),
            failure: Mutex::new(None),
            wake: Arc::new(tokio::sync::Notify::new()),
            running: tokio::sync::Mutex::new(()),
        }
    }

    /// Mark what committed events changed and wake the background pass. Takes
    /// no database work, so a save never waits on the unified store.
    /// `owner_of_memory` names the person whose memory an id is, as the state
    /// now has it (a restored memory's event names no person).
    pub fn follow(&self, es: &[Event], owner_of_memory: impl Fn(&str) -> Option<String>) {
        let mut touched = Marks::default();
        for e in es {
            match &e.ev {
                Ev::Memory { person, .. } | Ev::PersonErased { person } | Ev::Trashed { person, .. } => {
                    touched.people.insert(person.clone());
                }
                Ev::MemorySuperseded(p) => {
                    touched.people.insert(p.person.clone());
                }
                Ev::MemoryArchived(p) => {
                    touched.people.insert(p.person.clone());
                }
                Ev::Restored { item } => {
                    if let Some(person) = owner_of_memory(item) {
                        touched.people.insert(person);
                    }
                }
                Ev::MemoryForgotten { id } => {
                    touched.forgotten.insert(id.clone());
                }
                // A memory removed for good was trashed first, and the
                // `Trashed` already took it out of the person's current memory.
                _ => {}
            }
        }
        if touched.is_empty() {
            return;
        }
        {
            let mut synced = lock(&self.synced);
            for p in &touched.people {
                synced.remove(p);
            }
        }
        lock(&self.marks).merge(touched);
        self.wake.notify_one();
    }

    /// True when the unified store holds exactly `person`'s current memory.
    pub fn is_synced(&self, person: &str) -> bool {
        if lock(&self.marks).hold_back(person) || lock(&self.in_flight).hold_back(person) {
            return false;
        }
        lock(&self.synced).contains(person)
    }

    /// True while anything is marked.
    pub fn behind(&self) -> bool {
        !lock(&self.marks).is_empty()
    }

    /// Why the last pass failed, if none has succeeded since.
    pub fn failure(&self) -> Option<String> {
        lock(&self.failure).clone()
    }

    /// Take the marks for a pass; they stay in flight until `finish`.
    fn begin(&self) -> Marks {
        let todo = std::mem::take(&mut *lock(&self.marks));
        *lock(&self.in_flight) = todo.clone();
        todo
    }

    /// End a pass: what it could not do is marked again.
    fn finish(&self, undone: Marks) {
        lock(&self.marks).merge(undone);
        *lock(&self.in_flight) = Marks::default();
    }

    /// Record `person` as in step, unless a write marked them again meanwhile.
    /// They are served only once the pass has finished (`in_flight` holds them).
    fn settle(&self, person: &str) {
        let m = lock(&self.marks);
        if m.everything || m.people.contains(person) {
            return;
        }
        lock(&self.synced).insert(person.to_string());
    }
}

#[cfg(test)]
impl MemorySync {
    /// Mark everyone, as at start (tests create the tables after boot).
    pub(crate) fn mark_everything(&self) {
        lock(&self.marks).everything = true;
        self.wake.notify_one();
    }

    pub(crate) fn begin_for_test(&self) {
        self.begin();
    }

    pub(crate) fn finish_for_test(&self) {
        self.finish(Marks::default());
    }

    pub(crate) fn settle_for_test(&self, person: &str) {
        self.settle(person);
    }
}

/// Run a pass whenever a write wakes the sync, and every
/// `SYNC_RETRY_EVERY_SECS` while it is behind, for as long as the kernel runs.
/// The kernel is held only while a pass runs, never across the wait.
pub fn schedule(k: &Arc<Kernel>) {
    let weak = Arc::downgrade(k);
    let wake = k.memory_sync.wake.clone();
    tokio::spawn(async move {
        loop {
            {
                let Some(k) = weak.upgrade() else { return };
                if k.memory_sync.behind() {
                    if let Err(e) = k.sync_memory().await {
                        tracing::warn!("memory sync: {e:#}; tried again within {}s", SYNC_RETRY_EVERY_SECS.as_secs());
                    }
                }
            }
            tokio::select! {
                _ = wake.notified() => {}
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
    /// The ids of `person`'s current memory: in the library (not trashed or
    /// forgotten), not archived, not superseded.
    pub(crate) fn current_memory_ids(&self, person: &str) -> HashSet<String> {
        self.read(|s| {
            s.library
                .memory()
                .iter()
                .filter(|m| m.person == person && !s.memory_archived.contains(&m.id) && !s.memory_superseded.contains_key(&m.id))
                .map(|m| m.id.clone())
                .collect()
        })
    }

    /// One sync pass over what is marked, bounded by `SYNC_PASS_TIMEOUT_SECS`.
    /// What it could not finish stays marked.
    pub async fn sync_memory(&self) -> Result<()> {
        let _one = self.memory_sync.running.lock().await;
        let todo = self.memory_sync.begin();
        if todo.is_empty() {
            self.memory_sync.finish(Marks::default());
            return Ok(());
        }
        let (undone, failure) = match tokio::time::timeout(SYNC_PASS_TIMEOUT_SECS, self.sync_marked(&todo)).await {
            Ok((undone, failure)) => (undone, failure),
            Err(_) => (todo, Some(format!("the pass ran past {}s", SYNC_PASS_TIMEOUT_SECS.as_secs()))),
        };
        self.memory_sync.finish(undone);
        *lock(&self.memory_sync.failure) = failure.clone();
        match failure {
            Some(f) => anyhow::bail!("{f}"),
            None => Ok(()),
        }
    }

    /// Work off `todo`; returns what is left undone and why, if anything is.
    async fn sync_marked(&self, todo: &Marks) -> (Marks, Option<String>) {
        let cfg = &self.cfg.memory;
        let mut store = match MemoryStore::connect(&self.cfg.database.url, cfg).await {
            Ok(s) => s,
            Err(e) => return (todo.clone(), Some(format!("connect: {e:#}"))),
        };
        match store.tables_present().await {
            Ok(true) => {}
            // Tables still to be migrated for a mode that needs them: keep the marks.
            Ok(false) if cfg.is_unified_enabled() => return (todo.clone(), Some("the unified memory tables are missing".into())),
            // Legacy everywhere and no tables: nothing holds a copy to keep in step.
            Ok(false) => return (Marks::default(), None),
            Err(e) => return (todo.clone(), Some(format!("{e:#}"))),
        }

        let mut undone = Marks::default();
        let mut failures: Vec<String> = Vec::new();
        for id in &todo.forgotten {
            if let Err(e) = store.hard_delete_source_all_revs(&cfg.tenant_id, id).await {
                undone.forgotten.insert(id.clone());
                failures.push(format!("delete forgotten memory {id}: {e:#}"));
            }
        }
        let mut people = todo.people.clone();
        if todo.everything {
            people.extend(self.read(|s| s.library.memory().iter().map(|m| m.person.clone()).filter(|p| cfg.mode_for(p) != MemoryMode::Legacy).collect::<Vec<_>>()));
            match store.owners(&cfg.tenant_id).await {
                Ok(owners) => people.extend(owners),
                Err(e) => {
                    undone.everything = true;
                    failures.push(format!("list unified memory owners: {e:#}"));
                }
            }
        }
        for person in &people {
            match self.sync_person(&mut store, person).await {
                Ok(()) => self.memory_sync.settle(person),
                Err(e) => {
                    undone.people.insert(person.clone());
                    failures.push(format!("sync memory for {person}: {e:#}"));
                }
            }
        }
        let failure = (!failures.is_empty()).then(|| format!("{} of the pass failed; first: {}", failures.len(), failures[0]));
        (undone, failure)
    }

    /// Make `person`'s sources equal their current memory. Sources no longer
    /// current are deleted in every mode; missing ones are copied only when
    /// the person is not on Legacy. Holds the owner's erasure lock shared
    /// throughout, so an erasure cannot fall between reading their content
    /// and copying it.
    async fn sync_person(&self, store: &mut MemoryStore, person: &str) -> Result<()> {
        hold_owner_for_sync(store.client(), person).await?;
        let result = self.sync_person_held(store, person).await;
        release_owner_for_sync(store.client(), person).await?;
        result
    }

    async fn sync_person_held(&self, store: &mut MemoryStore, person: &str) -> Result<()> {
        let tenant = &self.cfg.memory.tenant_id;
        let current = self.current_memory_ids(person);
        let have = store.source_ids_for_owner(tenant, person).await?;
        for stale in have.difference(&current) {
            store.hard_delete_source_all_revs(tenant, stale).await?;
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
