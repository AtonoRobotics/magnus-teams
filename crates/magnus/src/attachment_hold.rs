//! A fetched attachment's bytes, held only while the run that fetched them
//! needs them (owner, 2026-11-14: "a temporary file that is deleted
//! automatically when complete"). They are never in the event log or the
//! state: a result records the attachment by name, type, size and SHA-256,
//! and its bytes stay here, in memory, until the run ends, an hour passes or
//! room is needed, whichever is first. The mailbox keeps the original, so a
//! run that needs one again fetches it again.
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longer than a run's work on its files takes; a run's end lets go first.
const HELD_FOR: Duration = Duration::from_secs(60 * 60);
/// Over this, the oldest go first.
const MAX_HELD_BYTES: usize = 1 << 30;

struct Held {
    run: String,
    name: String,
    data_b64: String,
    at: Instant,
}

static HELD: Mutex<Vec<Held>> = Mutex::new(Vec::new());

fn with<T>(f: impl FnOnce(&mut Vec<Held>) -> T) -> T {
    f(&mut HELD.lock().unwrap_or_else(|e| e.into_inner()))
}

fn prune(held: &mut Vec<Held>, now: Instant) {
    held.retain(|h| now.saturating_duration_since(h.at) < HELD_FOR);
    let mut total: usize = held.iter().map(|h| h.data_b64.len()).sum();
    while total > MAX_HELD_BYTES && !held.is_empty() {
        total -= held.remove(0).data_b64.len();
    }
}

/// Holds each attachment's bytes for `run` and gives back what the record
/// keeps of it: name, type, size and digest, no bytes.
pub fn hold(run: &str, found: Vec<Value>) -> Vec<Value> {
    hold_at(run, found, Instant::now())
}

fn hold_at(run: &str, found: Vec<Value>, now: Instant) -> Vec<Value> {
    let b64 = base64::engine::general_purpose::STANDARD;
    with(|held| {
        prune(held, now);
        found
            .into_iter()
            .map(|mut a| {
                let data = a.as_object_mut().and_then(|o| o.remove("data_b64")).and_then(|d| d.as_str().map(String::from)).unwrap_or_default();
                let bytes = b64.decode(&data).unwrap_or_default();
                a["size"] = json!(bytes.len());
                a["sha256"] = json!(hex::encode(Sha256::digest(&bytes)));
                let name = a["name"].as_str().unwrap_or_default().to_string();
                held.retain(|h| !(h.run == run && h.name == name));
                held.push(Held { run: run.to_string(), name, data_b64: data, at: now });
                a
            })
            .collect()
    })
}

/// The bytes `run` fetched as `name`, while they are held.
pub fn held(run: &str, name: &str) -> Option<String> {
    held_at(run, name, Instant::now())
}

fn held_at(run: &str, name: &str, now: Instant) -> Option<String> {
    with(|held| {
        prune(held, now);
        held.iter().rev().find(|h| h.run == run && h.name == name).map(|h| h.data_b64.clone())
    })
}

/// A run has ended: what it fetched goes with it.
pub fn release(run: &str) {
    with(|held| held.retain(|h| h.run != run));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(name: &str) -> Value {
        json!({ "name": name, "media_type": "image/jpeg", "data_b64": "SEVMTE8=", "source": "email" })
    }

    #[test]
    fn the_record_keeps_no_bytes_and_the_bytes_go_when_the_run_ends() {
        let kept = hold("run-hold-1", vec![photo("image0.jpeg")]);
        assert!(kept[0].get("data_b64").is_none(), "{kept:?}");
        assert_eq!((kept[0]["size"].as_u64(), kept[0]["sha256"].as_str().map(str::len)), (Some(5), Some(64)));
        assert_eq!(held("run-hold-1", "image0.jpeg").as_deref(), Some("SEVMTE8="));
        assert_eq!(held("run-hold-other", "image0.jpeg"), None, "another run read it");
        release("run-hold-1");
        assert_eq!(held("run-hold-1", "image0.jpeg"), None, "it outlived its run");
    }

    #[test]
    fn the_bytes_go_after_an_hour() {
        let then = Instant::now();
        hold_at("run-hold-2", vec![photo("a.jpeg")], then);
        assert!(held_at("run-hold-2", "a.jpeg", then + HELD_FOR / 2).is_some());
        assert_eq!(held_at("run-hold-2", "a.jpeg", then + HELD_FOR), None, "it outlived its hour");
    }
}
