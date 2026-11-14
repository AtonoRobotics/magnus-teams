use crate::log::Event;
use crate::state::State;
use serde_json::{json, Value};
use std::collections::HashSet;

fn ev(seq: i64, actor: &str, ev: Value) -> Event {
    Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
}

/// Bob's mail inbox window after replaying `answers`, each `(ids, full)`.
fn replay(answers: &[(&[&str], Option<bool>)]) -> Vec<HashSet<String>> {
    let mut s = State::default();
    for (i, (ids, full)) in answers.iter().enumerate() {
        let mut w = json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids });
        if let Some(full) = full {
            w["full"] = json!(full);
        }
        s.apply(&ev(i as i64 + 1, "person:bob", w));
    }
    s.watch.window("bob", "mail", "list_inbox").cloned().unwrap_or_default()
}

fn set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Pre-#411 history holds fresh-only ids after the seed (#415):
/// replay unions each onto the newest entry, reconstructing the
/// answers instead of misreading deltas as wholes.
#[test]
fn old_delta_events_replay_as_full_answers() {
    let window = replay(&[(&["a"], None), (&["b"], None), (&["c"], None)]);
    assert_eq!(window.len(), 2, "replay kept {window:?}");
    assert_eq!(window[0], set(&["a", "b"]), "the middle answer lost its seed");
    assert_eq!(window[1], set(&["a", "b", "c"]), "the last answer lost its history");
}

/// New events carry whole answers (#415): replay pushes them as they
/// come, with no unioning.
#[test]
fn full_answer_events_replay_whole() {
    let window = replay(&[(&["a"], Some(true)), (&["b"], Some(true))]);
    assert_eq!(window.len(), 2, "replay kept {window:?}");
    assert_eq!(window[0], set(&["a"]));
    assert_eq!(window[1], set(&["b"]));
}

/// Era-2 history (whole answers, untagged, #423): an untagged event
/// overlapping the window pushes whole instead of unioning.
#[test]
fn overlapping_untagged_events_replay_whole() {
    let window = replay(&[(&["a"], None), (&["a", "b"], None), (&["b"], None)]);
    assert_eq!(window.len(), 2, "replay kept {window:?}");
    assert_eq!(window[0], set(&["a", "b"]));
    assert_eq!(window[1], set(&["b"]), "the overlapping answer unioned");
}

/// Mixed history across the upgrade (#423): era-1 deltas union,
/// then an overlapping era-2 whole pushes.
#[test]
fn mixed_delta_then_whole_history_replays_each_as_itself() {
    let window = replay(&[(&["a"], None), (&["b"], None), (&["b", "c"], None)]);
    assert_eq!(window.len(), 2, "replay kept {window:?}");
    assert_eq!(window[0], set(&["a", "b"]));
    assert_eq!(window[1], set(&["b", "c"]), "the whole answer unioned onto history");
}

/// The accepted edge (#423): a total-churn era-2 pair is
/// indistinguishable from seed plus delta, so it unions. Two
/// tagged reads heal the window; this pins the transient shape.
#[test]
fn disjoint_untagged_era_two_pair_unions_transiently() {
    let window = replay(&[(&["a"], None), (&["b"], None)]);
    assert_eq!(window.len(), 2, "replay kept {window:?}");
    assert_eq!(window[0], set(&["a"]));
    assert_eq!(window[1], set(&["a", "b"]), "the disjoint pair did not union");
}

/// A scheduled poll restarts the read's clock; another read's stays.
#[test]
fn a_poll_restarts_only_its_own_clock() {
    let mut s = State::default();
    let polled = ev(1, "kernel", json!({ "kind": "watch_polled", "person": "bob", "app": "mail", "tool": "list_inbox" }));
    s.apply(&polled);
    assert_eq!(s.watch.last_polled("bob", "mail", "list_inbox"), Some(polled.at));
    assert_eq!(s.watch.last_polled("bob", "mail", "list_sent"), None);
}
