//! What Magnus asks a computer's guest agent (`sandbox/guest`), one JSON
//! line each, over the stream a [`crate::Request::Connect`] opens. Computerd
//! only relays these: it never reads them.
//!
//! The guest answers with a JSON object of its own, `{"error": ...}` when it
//! could not. What it sends back is untrusted and stays a JSON value for the
//! caller to read. The guest reads the fields it knows and ignores the rest,
//! so a field left out here (`None`) is left off the wire, as before.
//!
//! A browser VM's agent is asked through the same stream, but its requests
//! are mostly the model's own tool arguments passed through, so they are not
//! typed here.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GuestRequest {
    /// Whether the agent answers.
    Ping,
    /// Flushes the guest's writes to its volume.
    Sync,
    /// A shell command, stopped by the guest after `timeout_s` seconds.
    Exec { command: String, timeout_s: u64 },
    /// A file, whole, or `length` bytes from `offset`, in base64.
    Read {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u64>,
    },
    /// Writes a file, or adds to its end with `append`.
    Write {
        path: String,
        data_b64: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<bool>,
    },
    /// A folder's entries.
    List { path: String },
    /// Deletes a file or folder.
    Erase { path: String },
    /// Zeroes the volume's free space, so a deleted file's blocks hold nothing.
    ZeroFree,
    /// A person takes the desktop: the guest notes what already runs.
    TakeoverBegin,
    /// The person hands back: what they started is stopped.
    TakeoverEnd,
    /// What a handback would stop, without stopping it.
    TakeoverLeftovers,
}

impl GuestRequest {
    /// A whole file.
    pub fn read(path: impl Into<String>) -> Self {
        GuestRequest::Read { path: path.into(), offset: None, length: None }
    }

    /// A new file, or one replaced.
    pub fn write(path: impl Into<String>, data_b64: impl Into<String>) -> Self {
        GuestRequest::Write { path: path.into(), data_b64: data_b64.into(), append: None }
    }

    pub fn exec(command: impl Into<String>, timeout_s: u64) -> Self {
        GuestRequest::Exec { command: command.into(), timeout_s }
    }
}
