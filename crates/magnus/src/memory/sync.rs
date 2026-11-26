//! Keeps the unified store holding exactly each person's current memory,
//! which is what recall searches.
//!
//! Every write that changes someone's memory updates the store as part of
//! `Kernel::emit_all` (`Kernel::mirror_memory`): a save or restore copies the
//! memory in, and a forget, trash, archive or supersession deletes its copy.
//! At boot `Kernel::mirror_all_memory` brings the store level with the state
//! once, for appliances that had memory before the store and for anything a
//! failed write left behind. The store is in the same database as the event
//! log, so a failure here is a failure of the write.

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::log::Event;
use crate::memory::ingest::ingest_source;
use crate::memory::store::MemoryStore;
use crate::memory::{SourceInput, TaintLabel};
use crate::recall::Remembered;
use anyhow::{Context, Result};
use std::collections::HashSet;

fn format_memory_text(fact: &str, topic: Option<&str>) -> String {
    match topic.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("[{t}] {fact}"),
        None => fact.to_string(),
    }
}

/// What a committed event does to the store: memories to copy in, and
/// memory ids whose copies to delete.
fn changes(es: &[Event], is_memory: impl Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
    let (mut copy, mut delete) = (Vec::new(), Vec::new());
    for e in es {
        match &e.ev {
            Ev::Memory { id, .. } => copy.push(id.clone()),
            Ev::Restored { item } if is_memory(item) => copy.push(item.clone()),
            Ev::MemoryForgotten { id } => delete.push(id.clone()),
            Ev::Trashed { item, .. } => delete.push(item.clone()),
            Ev::MemoryArchived(p) => delete.push(p.id.clone()),
            Ev::MemorySuperseded(p) => delete.push(p.old_id.clone()),
            _ => {}
        }
    }
    (copy, delete)
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

    /// Apply what committed events changed in people's memory to the store.
    pub(crate) async fn mirror_memory(&self, es: &[Event]) -> Result<()> {
        let (copy, delete) = changes(es, |id| self.read(|s| s.library.memory().iter().any(|m| m.id == id)));
        if copy.is_empty() && delete.is_empty() {
            return Ok(());
        }
        let mut store = MemoryStore::connect(&self.cfg.database.url, &self.cfg.memory).await?;
        let tenant = &self.cfg.memory.tenant_id;
        for id in &delete {
            store.hard_delete_source_all_revs(tenant, id).await.with_context(|| format!("delete the copy of memory {id}"))?;
        }
        let owners: Vec<(String, String)> = self.read(|s| s.library.memory().iter().filter(|m| copy.contains(&m.id)).map(|m| (m.id.clone(), m.person.clone())).collect());
        for (id, person) in owners {
            self.copy_memories(&mut store, &person, &HashSet::from([id])).await?;
        }
        Ok(())
    }

    /// Bring the store level with everyone's current memory: copy what is
    /// missing and delete what is no longer current, including the copies of
    /// people who have none left.
    pub(crate) async fn mirror_all_memory(&self) -> Result<()> {
        let mut store = MemoryStore::connect(&self.cfg.database.url, &self.cfg.memory).await?;
        let tenant = self.cfg.memory.tenant_id.clone();
        let mut people: HashSet<String> = self.read(|s| s.library.memory().iter().map(|m| m.person.clone()).collect());
        people.extend(store.owners(&tenant).await?);
        for person in &people {
            let current = self.current_memory_ids(person);
            let have = store.source_ids_for_owner(&tenant, person).await?;
            for stale in have.difference(&current) {
                store.hard_delete_source_all_revs(&tenant, stale).await?;
            }
            let missing: HashSet<String> = current.difference(&have).cloned().collect();
            self.copy_memories(&mut store, person, &missing).await.with_context(|| format!("copy {person}'s memory"))?;
        }
        Ok(())
    }

    /// Copy `ids` of `person`'s memory into the store, read as recall reads it.
    async fn copy_memories(&self, store: &mut MemoryStore, person: &str, ids: &HashSet<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let tenant = &self.cfg.memory.tenant_id;
        let to_copy: Vec<Remembered> = self.remembered(person, |m| ids.contains(&m.id)).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{MemoryArchivedPayload, MemorySupersededPayload};
    use crate::tools::Label;

    fn event(ev: Ev) -> Event {
        Event { seq: 1, at: chrono::Utc::now(), actor: "person:mo".into(), ev }
    }

    #[test]
    fn each_memory_change_copies_or_deletes() {
        let es = [
            event(Ev::Memory { id: "m1".into(), person: "mo".into(), run: String::new(), fact: "f".into(), topic: None, label: Label::default(), sources: vec![], by: None }),
            event(Ev::Restored { item: "m2".into() }),
            event(Ev::Restored { item: "file_9".into() }),
            event(Ev::MemoryForgotten { id: "m3".into() }),
            event(Ev::Trashed { item: "m4".into(), person: "mo".into() }),
            event(Ev::MemoryArchived(MemoryArchivedPayload { id: "m5".into(), person: "mo".into(), reason: "cold".into() })),
            event(Ev::MemorySuperseded(MemorySupersededPayload {
                old_id: "m6".into(),
                new_id: "m1".into(),
                person: "mo".into(),
                reason: "newer".into(),
                valid_until: chrono::Utc::now(),
            })),
        ];
        let (copy, delete) = changes(&es, |id| id.starts_with('m'));
        assert_eq!(copy, vec!["m1", "m2"], "a restored file is not a memory");
        assert_eq!(delete, vec!["m3", "m4", "m5", "m6"]);
    }
}
