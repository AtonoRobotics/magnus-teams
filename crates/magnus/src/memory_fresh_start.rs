//! Everyone's memory starts empty at the memory update. The owner,
//! 2026-11-27: "Start everyone empty", and "The old memory should not be
//! used as of the new memory update." What was filed before it, under the
//! old rules (status reports, run checkpoints, duplicates), stays in the
//! log as history and is never served: each is archived once, at boot.

use crate::events::{Ev, MemoryArchivedPayload};
use crate::kernel::Kernel;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// The memory update: a memory written before it is archived at boot.
/// 2026-11-27T18:47:00Z, in seconds since the Unix epoch: when the
/// person-only write rules (3d0e543, test.392) went live on the server.
pub const MEMORY_FRESH_START: DateTime<Utc> = match DateTime::from_timestamp(1_795_805_220, 0) {
    Some(at) => at,
    None => panic!("MEMORY_FRESH_START is not a time"),
};

/// The reason a fresh start's archive records.
pub const FRESH_START_REASON: &str = "fresh_start";

impl Kernel {
    /// Archive every memory written before `cutoff` that is still current,
    /// in one batch, and say how many. A memory in the trash is archived
    /// too, so putting it back does not serve it again. Run at boot, after
    /// replay and before serving; once done, a later boot finds nothing.
    pub async fn archive_memory_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let old: BTreeMap<String, String> = self.read(|s| {
            s.library
                .memory()
                .iter()
                .chain(s.trash.items().values().filter_map(|t| t.memory()))
                .filter(|m| m.at < cutoff && !s.memory_retired(&m.id))
                .map(|m| (m.id.clone(), m.person.clone()))
                .collect()
        });
        let n = old.len();
        if n > 0 {
            let evs = old.into_iter().map(|(id, person)| ("kernel", Ev::MemoryArchived(MemoryArchivedPayload { id, person, reason: FRESH_START_REASON.into() }))).collect();
            self.emit_all(evs).await?;
        }
        Ok(n)
    }
}

#[cfg(test)]
#[path = "memory_fresh_start_tests.rs"]
mod tests;
