//! Memory lifecycle event payloads (W2.3, W3.1, N10, B3).
//!
//! Kept in `events/memory.rs` (matching `events/meta.rs`) so `events.rs`
//! shrinks while wire tags (`memory`, `memory_pass`, `memory_forgotten`,
//! `memory_superseded`, `memory_archived`, `prefetch_recorded`) remain
//! top-level and strictly backward-compatible.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Records that an older personal memory entry was superseded by a newer one
/// (bi-temporal validity, W3.1). Nothing writes it since the curator was
/// retired; a logged one is still read, and the old entry is not served.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MemorySupersededPayload {
    pub old_id: String,
    pub new_id: String,
    pub person: String,
    #[serde(default)]
    pub reason: String,
    pub valid_until: DateTime<Utc>,
}

/// Records that a personal memory entry was archived: kept in the log as
/// history, never served again. Written today only by the fresh start
/// (`memory_fresh_start`, reason `"fresh_start"`); an older log may hold
/// the retired curator's Tier-2 ageing (`"tier2_cold_after_30_days"`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MemoryArchivedPayload {
    pub id: String,
    pub person: String,
    #[serde(default)]
    pub reason: String,
}
