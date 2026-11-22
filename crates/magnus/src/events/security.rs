//! Security event payloads persisted in the event log (W1.4, B3).
//!
//! Each payload is carried by its own top-level `Ev` variant so older binaries
//! skip unknown top-level kinds cleanly on rollback (`log.rs:751`).

use serde::{Deserialize, Serialize};

/// Records a tainted web query charge or denial against a turn/firing scope.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QueryGatePayload {
    pub scope: String,
    #[serde(default)]
    pub bytes_added: usize,
    #[serde(default)]
    pub denied: bool,
}

/// Records a blocked outbound send so resubmission checks survive restarts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SafetyBlockedPayload {
    pub run: String,
    pub tool: String,
    pub hash: String,
    pub reason: String,
}
