//! A fetched attachment's bytes, held only while the run that fetched them
//! needs them (owner, 2026-11-14: "a temporary file that is deleted
//! automatically when complete"). They are never in the event log or the
//! state: a result records the attachment by name, type, size and SHA-256,
//! and its bytes stay here, in memory, known by that digest (mail reuses
//! names: two `image0.jpeg`s are two attachments), until the run ends, an hour passes or
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
/// Of the SHA-256 in an attachment's reference: enough to tell apart the
/// attachments one task reads.
const REF_HEX_CHARS: usize = 8;

struct Held {
    run: String,
    sha256: String,
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

/// How the model names an attachment however many share its name: the name
/// and the first hex of its SHA-256, `image0.jpeg#1a2b3c4d`.
pub fn reference(name: &str, sha256: &str) -> String {
    format!("{name}#{}", sha256.get(..REF_HEX_CHARS).unwrap_or(sha256))
}

/// Holds each attachment's bytes for `run` and gives back what the record
/// keeps of it: name, type, size, digest and reference, no bytes.
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
                let sha256 = hex::encode(Sha256::digest(&bytes));
                a["sha256"] = json!(sha256);
                a["ref"] = json!(reference(a["name"].as_str().unwrap_or_default(), &sha256));
                held.retain(|h| !(h.run == run && h.sha256 == sha256));
                held.push(Held { run: run.to_string(), sha256, data_b64: data, at: now });
                a
            })
            .collect()
    })
}

/// The bytes `run` fetched with this SHA-256, while they are held.
pub fn held(run: &str, sha256: &str) -> Option<String> {
    held_at(run, sha256, Instant::now())
}

/// A read drops nothing: bytes past their hour read as gone, and the next
/// hold prunes them. So no read can take another run's bytes with it.
fn held_at(run: &str, sha256: &str, now: Instant) -> Option<String> {
    with(|held| held.iter().rev().find(|h| h.run == run && h.sha256 == sha256 && now.saturating_duration_since(h.at) < HELD_FOR).map(|h| h.data_b64.clone()))
}

/// A run has ended: what it fetched goes with it.
pub fn release(run: &str) {
    with(|held| held.retain(|h| h.run != run));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(name: &str) -> Value {
        file(name, "SEVMTE8=")
    }

    fn file(name: &str, b64: &str) -> Value {
        json!({ "name": name, "media_type": "image/jpeg", "data_b64": b64, "source": "email" })
    }

    fn sha(kept: &Value) -> String {
        kept["sha256"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn the_record_keeps_no_bytes_and_the_bytes_go_when_the_run_ends() {
        let kept = hold("run-hold-1", vec![photo("image0.jpeg")]);
        assert!(kept[0].get("data_b64").is_none(), "{kept:?}");
        assert_eq!((kept[0]["size"].as_u64(), kept[0]["sha256"].as_str().map(str::len)), (Some(5), Some(64)));
        assert_eq!(kept[0]["ref"], json!(format!("image0.jpeg#{}", &sha(&kept[0])[..REF_HEX_CHARS])));
        assert_eq!(held("run-hold-1", &sha(&kept[0])).as_deref(), Some("SEVMTE8="));
        assert_eq!(held("run-hold-other", &sha(&kept[0])), None, "another run read it");
        release("run-hold-1");
        assert_eq!(held("run-hold-1", &sha(&kept[0])), None, "it outlived its run");
    }

    /// Two attachments with one name, from two messages, are both held,
    /// each as its own bytes.
    #[test]
    fn two_attachments_with_one_name_are_both_held() {
        let first = hold("run-hold-5", vec![file("image0.jpeg", "Rmlyc3Q=")]);
        let second = hold("run-hold-5", vec![file("image0.jpeg", "U2Vjb25k")]);
        assert_ne!(first[0]["ref"], second[0]["ref"], "two attachments shared a reference");
        assert_eq!(held("run-hold-5", &sha(&first[0])).as_deref(), Some("Rmlyc3Q="), "the second overwrote the first");
        assert_eq!(held("run-hold-5", &sha(&second[0])).as_deref(), Some("U2Vjb25k"));
        release("run-hold-5");
    }

    #[test]
    fn the_bytes_go_after_an_hour() {
        let then = Instant::now();
        let kept = hold_at("run-hold-2", vec![photo("a.jpeg")], then);
        assert!(held_at("run-hold-2", &sha(&kept[0]), then + HELD_FOR / 2).is_some());
        assert_eq!(held_at("run-hold-2", &sha(&kept[0]), then + HELD_FOR), None, "it outlived its hour");
    }

    /// A read with a later clock answers for its own run only: it took the
    /// whole store with it, and another run's bytes went before their hour.
    #[test]
    fn a_late_read_leaves_another_runs_bytes() {
        let now = Instant::now();
        let kept = hold_at("run-hold-3", vec![photo("b.jpeg")], now);
        assert_eq!(held_at("run-hold-4", &sha(&kept[0]), now + HELD_FOR * 2), None);
        assert_eq!(held("run-hold-3", &sha(&kept[0])).as_deref(), Some("SEVMTE8="), "a read of another run dropped these bytes");
        release("run-hold-3");
    }
}
