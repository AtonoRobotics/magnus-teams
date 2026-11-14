//! What Magnus says to `magnus-computerd`, defined once for both
//! (`decisions/2026-11-14-typed-computerd-protocol.md`).
//!
//! Transport: computerd listens on a Unix socket. Each connection carries one
//! JSON line, a [`Request`], and gets one JSON line back: the answer's fields
//! with `"ok": true`, or `{"ok": false, "error": ...}`. A [`Request::Connect`]
//! or [`Request::Display`] instead turns the connection, after an `ok` line,
//! into a stream to the computer's guest agent or its desktop. What the guest
//! agent is asked is in [`guest`].
//!
//! Computerd is root's and is updated apart from the server
//! (`decisions/2026-11-11-computerd-version-and-update.md`), so the JSON here
//! is the wire both sides already speak, and a test holds each request to it.

pub mod guest;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// One request to computerd. It carries a computer id and an operation,
/// never a path, a command or a size: unknown fields are refused, and an
/// unknown `op` is refused as a bad request.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Create {
        id: String,
    },
    Start {
        id: String,
    },
    Hibernate {
        id: String,
    },
    Wake {
        id: String,
    },
    DiscardSnapshot {
        id: String,
    },
    Backup {
        id: String,
    },
    /// Where the egress proxy listens.
    EgressSocket,
    /// How many computers run and exist, against the machine's limits.
    Capacity,
    /// The computer that runs as this uid.
    Whose {
        uid: u32,
    },
    RestoreBackup {
        id: String,
    },
    /// A browser VM: the volume is its profile, and there is no shell.
    StartBrowser {
        id: String,
    },
    /// Whether the host can hold this volume with every block written.
    ZeroRoom {
        id: String,
    },
    /// Gives a stopped computer's zeroed blocks back to the host.
    DigHoles {
        id: String,
    },
    /// Encrypts a stopped browser's profile with a key the vault holds.
    SealVolume {
        id: String,
        key_b64: String,
    },
    UnsealVolume {
        id: String,
        key_b64: String,
    },
    /// A VM with only the volume, to reach a stopped computer's files.
    StartHelper {
        id: String,
        #[serde(default)]
        read_only: bool,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Stop {
        id: String,
    },
    Destroy {
        id: String,
        #[serde(default)]
        delete_volume: bool,
    },
    /// A reset that erases the files: the same computer with a new, empty
    /// volume. No path or size comes from the caller.
    EraseVolume {
        id: String,
    },
    Status {
        id: String,
    },
    List,
    /// Turns this connection into a stream to the guest agent.
    Connect {
        id: String,
    },
    /// The computer's desktop, as a raw stream to the guest.
    Display {
        id: String,
    },
    /// This build's release, base image and requests, for Magnus to compare
    /// with the computerd it shipped with. A computerd older than this
    /// request answers "bad request", which Magnus reads as older.
    Version,
}

/// Every request, by its `op`: what a computerd says it serves in its
/// [`VersionReply`]. A test holds it to [`Request`], so none is left out.
pub const OPS: &[&str] = &[
    "create",
    "start",
    "hibernate",
    "wake",
    "discard_snapshot",
    "backup",
    "egress_socket",
    "capacity",
    "whose",
    "restore_backup",
    "start_browser",
    "zero_room",
    "dig_holes",
    "seal_volume",
    "unseal_volume",
    "start_helper",
    "pause",
    "resume",
    "stop",
    "destroy",
    "erase_volume",
    "status",
    "list",
    "connect",
    "display",
    "version",
];

impl Request {
    /// The request's `op`, as it goes on the wire.
    pub fn op(&self) -> &'static str {
        match self {
            Request::Create { .. } => "create",
            Request::Start { .. } => "start",
            Request::Hibernate { .. } => "hibernate",
            Request::Wake { .. } => "wake",
            Request::DiscardSnapshot { .. } => "discard_snapshot",
            Request::Backup { .. } => "backup",
            Request::EgressSocket => "egress_socket",
            Request::Capacity => "capacity",
            Request::Whose { .. } => "whose",
            Request::RestoreBackup { .. } => "restore_backup",
            Request::StartBrowser { .. } => "start_browser",
            Request::ZeroRoom { .. } => "zero_room",
            Request::DigHoles { .. } => "dig_holes",
            Request::SealVolume { .. } => "seal_volume",
            Request::UnsealVolume { .. } => "unseal_volume",
            Request::StartHelper { .. } => "start_helper",
            Request::Pause { .. } => "pause",
            Request::Resume { .. } => "resume",
            Request::Stop { .. } => "stop",
            Request::Destroy { .. } => "destroy",
            Request::EraseVolume { .. } => "erase_volume",
            Request::Status { .. } => "status",
            Request::List => "list",
            Request::Connect { .. } => "connect",
            Request::Display { .. } => "display",
            Request::Version => "version",
        }
    }
}

/// The answer to [`Request::Version`], beside `"ok": true`. Read leniently:
/// a field a computerd leaves out is empty, never an error.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionReply {
    /// The release it was installed from.
    pub version: String,
    /// The sha256 of its base image, when it has one.
    pub base_id: Option<String>,
    /// Every request it serves, by its `op`.
    pub ops: Vec<String>,
}
