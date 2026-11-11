//! What this computerd is, so Magnus can tell when it is older than the one
//! Magnus shipped with (`decisions/2026-11-11-computerd-version-and-update.md`).
//! Magnus's own updates do not reach this service: root installs it.

use serde_json::{json, Value};
use std::path::Path;

/// The release's version, written beside the program by the installer.
pub const VERSION_FILE: &str = "VERSION";
const MAX_VERSION_CHARS: usize = 64;

/// Every request this computerd answers, by its `op`. A test holds it to
/// the `Request` enum, so a new request cannot be left out.
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

/// The release this program came from: the VERSION the installer put beside
/// it, or the version it was built as when it was installed without one.
pub fn installed(program_dir: Option<&Path>) -> String {
    program_dir
        .and_then(|d| std::fs::read_to_string(d.join(VERSION_FILE)).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.len() <= MAX_VERSION_CHARS && v.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The answer to `version`: the release, the base image's id (the sha256
/// of `rootfs.ext4` as installed) and the requests this build knows.
pub fn answer(program_dir: Option<&Path>, base_id: Option<String>) -> Value {
    json!({ "version": installed(program_dir), "base_id": base_id, "ops": OPS })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Request;

    /// serde names every variant when it refuses an unknown one.
    fn every_request() -> Vec<String> {
        let e = serde_json::from_str::<Request>(r#"{"op":"no_such_request"}"#).unwrap_err().to_string();
        let list = e.split("expected one of").nth(1).expect("serde lists the variants");
        list.split(',').map(|s| s.split('`').nth(1).expect("a quoted name").to_string()).collect()
    }

    #[test]
    fn ops_names_every_request() {
        let mut ops: Vec<String> = OPS.iter().map(|s| s.to_string()).collect();
        let mut all = every_request();
        ops.sort();
        all.sort();
        assert_eq!(ops, all);
    }

    #[test]
    fn version_answers_the_release_the_base_image_and_the_requests() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(VERSION_FILE), "0.2.6-test.81\n").unwrap();
        let v = answer(Some(d.path()), Some("abc".into()));
        assert_eq!(v["version"], "0.2.6-test.81");
        assert_eq!(v["base_id"], "abc");
        assert!(v["ops"].as_array().unwrap().iter().any(|o| o == "erase_volume"));
        let bare = answer(None, None);
        assert_eq!(bare["version"], env!("CARGO_PKG_VERSION"), "without a VERSION file it says what it was built as");
        assert!(bare["base_id"].is_null());
    }

    #[test]
    fn a_version_file_that_is_not_a_version_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(VERSION_FILE), "1.0\n\"; rm -rf /").unwrap();
        assert_eq!(installed(Some(d.path())), env!("CARGO_PKG_VERSION"));
    }
}
