//! Memory lifecycle event payloads (W2.3, W3.1, N10, B3).
//!
//! Kept in `events/memory.rs` (matching `events/meta.rs`) so `events.rs`
//! shrinks while wire tags (`memory`, `memory_pass`, `memory_forgotten`,
//! `memory_superseded`, `memory_archived`, `prefetch_recorded`) remain
//! top-level and strictly backward-compatible.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Records that an older personal memory entry was superseded by a newer one
/// (bi-temporal validity, W3.1).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MemorySupersededPayload {
    pub old_id: String,
    pub new_id: String,
    pub person: String,
    #[serde(default)]
    pub reason: String,
    pub valid_until: DateTime<Utc>,
}

/// Records that a personal memory entry was moved to cold archive (e.g. Tier-2
/// ageing after `TIER2_COLD_DAYS` per O2, without deleting user data).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MemoryArchivedPayload {
    pub id: String,
    pub person: String,
    #[serde(default)]
    pub reason: String,
}
