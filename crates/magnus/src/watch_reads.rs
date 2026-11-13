//! Watched reads of an app (#369, #406): which items an answer holds,
//! the arguments a poll calls the read with, and what makes a watch well
//! formed. A catalog app declares its own (`watch` in `catalog/*.toml`),
//! so a connected app is polled without anyone configuring it
//! (decisions/2026-11-10-catalog-watched-reads.md). A poll is the
//! kernel's own read, not a model run (decisions/2026-11-11-polls-read-directly.md).

use crate::config::Watch;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

/// Joins the fields of a composite id (`event_id+changed`): an item whose
/// any part changes counts as new, so a changed event or a thread with a
/// new reply wakes like a new one.
pub const ID_JOIN: char = '+';

/// Item ids from a watched answer: the array at `items` (a JSON pointer,
/// `""` for the answer itself), each item's `id` (one field, or several
/// joined by `+`). None when the answer has no array there, which a
/// watch that does not match its read's answer shows as. Items lacking
/// any part of the id are skipped, never waked on.
pub fn ids(data: &Value, items: &str, id: &str) -> Option<Vec<String>> {
    let list = data.pointer(items)?.as_array()?;
    let part = |item: &Value, f: &str| match &item[f] {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    Some(list.iter().filter_map(|item| id.split(ID_JOIN).map(|f| part(item, f)).collect::<Option<Vec<_>>>().map(|p| p.join("\u{1f}"))).collect())
}

/// A time a watch's arguments name relative to the poll: `{now}`, or
/// `{now-<n>d}`, `{now+<n>h}` and the like (days `d`, hours `h`).
fn time(placeholder: &str, now: DateTime<Utc>) -> Option<String> {
    let inner = placeholder.strip_prefix("{now")?.strip_suffix('}')?;
    if inner.is_empty() {
        return Some(now.to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    let (sign, rest) = match inner.split_at(1) {
        ("+", r) => (1, r),
        ("-", r) => (-1, r),
        _ => return None,
    };
    let (n, unit) = rest.split_at(rest.len().checked_sub(1)?);
    let n: i64 = n.parse().ok()?;
    let d = match unit {
        "d" => Duration::days(n),
        "h" => Duration::hours(n),
        _ => return None,
    };
    Some((now + d * sign).to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The watch's arguments as a poll at `now` sends them: every string that
/// is a time placeholder becomes that time. None when the watch names no
/// arguments.
pub fn input(w: &Watch, now: DateTime<Utc>) -> Option<Value> {
    fn walk(v: &Value, now: DateTime<Utc>) -> Value {
        match v {
            Value::String(s) if s.starts_with("{now") => time(s, now).map(Value::String).unwrap_or_else(|| v.clone()),
            Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), walk(v, now))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|v| walk(v, now)).collect()),
            _ => v.clone(),
        }
    }
    w.input.as_ref().map(|v| walk(v, now))
}

/// How many times one poll calls its read before waiting for the next
/// interval: the read and one retry (#419).
pub const POLL_READ_ATTEMPTS: u32 = 2;
/// How long a poll waits before reading again after a failed read.
pub const POLL_RETRY_WAIT: std::time::Duration = std::time::Duration::from_secs(POLL_RETRY_WAIT_SECS);
const POLL_RETRY_WAIT_SECS: u64 = 2;

/// What a poll at `now` sends the read: the watch's arguments, or none.
pub fn args(w: &Watch, now: DateTime<Utc>) -> Value {
    input(w, now).unwrap_or_else(|| json!({}))
}

impl crate::kernel::Kernel {
    /// Poll one watched read for the person: call it once, exactly with
    /// the watch's arguments, through their own sign-in, with no model
    /// and no task. The read is authorised as the gate authorises any
    /// (`Kernel::authorised_read`): a read the app classes, no switch or
    /// turn-off holding the person's work on the app, their right to it,
    /// its screened definition and policy. The answer goes only to the
    /// diff, which keeps ids, so nothing the app returns reaches a model
    /// here; a review it wakes reads the items itself, through the boundary.
    pub(crate) async fn poll_read(self: &Arc<Self>, person: &str, app: &str, w: &Watch, now: DateTime<Utc>) -> Result<(), String> {
        use crate::authorised_read::Refused;
        let c = self.connector(app).ok_or("the app is no longer offered")?;
        let p = self.person(person).ok_or("no such person")?;
        let read = crate::authorised_read::SideRead { person: &p, app, read: &w.tool, args: args(w, now), within: None };
        let data = self.authorised_read(read).await.map_err(|r| match r {
            Refused::Unknown => "the watched tool is not one of the app's".into(),
            Refused::NotARead => "the watched tool is not a read".into(),
            Refused::Held(h) => h.said(),
            Refused::NoRights => "the person may not use the app".into(),
            Refused::Pin(why) | Refused::Failed(why) | Refused::Secret(why) | Refused::Unjudged(why) => why,
            Refused::Denied(reason) => format!("policy forbids the poll: {reason}"),
            Refused::Asks(rule) => format!("policy asks before the poll ({rule}), and no one is there to ask"),
            Refused::NotConnected => "the person is not connected to the app".into(),
            Refused::Call(e) => format!("{e:?}"),
            Refused::Audit(e) => format!("{e:#}"),
        })?;
        self.note_watched(person, app, &w.tool, &c.watch, &data).await;
        Ok(())
    }
}

/// What is wrong with an app's watches, if anything: each watches a read
/// the app classes, once (its poll clock and seen items are kept by
/// tool), points at its items, names each item's id, polls at least each
/// minute when it polls, and names its arguments as an object whose time
/// placeholders all read.
pub fn fault(app: &str, watches: &[Watch], class_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut tools = std::collections::HashSet::new();
    for w in watches {
        let bad = |why: &str| Some(format!("{app}: the watch of {}: {why}", w.tool));
        if !tools.insert(w.tool.as_str()) {
            return bad("watched twice");
        }
        match class_of(&w.tool).as_deref() {
            Some("read") => {}
            Some(c) => return bad(&format!("is {c}, and only a read is watched")),
            None => return bad("is not one of the app's tools"),
        }
        if !(w.items.is_empty() || w.items.starts_with('/')) {
            return bad("items is a JSON pointer");
        }
        if w.id.is_empty() || w.id.split(ID_JOIN).any(str::is_empty) {
            return bad("names no id");
        }
        if w.poll_minutes == Some(0) {
            return bad("polls every zero minutes");
        }
        match &w.input {
            None => {}
            Some(Value::Object(m)) => {
                if let Some(s) = m.values().filter_map(Value::as_str).find(|s| s.starts_with("{now") && time(s, Utc::now()).is_none()) {
                    return bad(&format!("{s} is not a time"));
                }
            }
            Some(_) => return bad("its arguments are an object"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn w(tool: &str, items: &str, id: &str, input: Option<Value>) -> Watch {
        Watch { tool: tool.into(), items: items.into(), id: id.into(), poll_minutes: Some(5), input }
    }

    #[test]
    fn ids_read_the_pointed_array() {
        let data = json!({ "messages": [{ "subject": "a" }, { "subject": "b" }, { "nope": true }, { "subject": 7 }] });
        assert_eq!(ids(&data, "/messages", "subject"), Some(vec!["a".into(), "b".into(), "7".into()]));
        assert_eq!(ids(&data, "/missing", "subject"), None);
        assert_eq!(ids(&data, "", "subject"), None);
        assert_eq!(ids(&json!([{ "id": "x" }]), "", "id"), Some(vec!["x".into()]));
    }

    /// A composite id changes when any part does: a changed event is new.
    #[test]
    fn a_composite_id_changes_with_any_part() {
        let before = ids(&json!({ "events": [{ "event_id": "e1", "changed": "t1" }, { "event_id": "e2" }] }), "/events", "event_id+changed").unwrap();
        let after = ids(&json!({ "events": [{ "event_id": "e1", "changed": "t2" }] }), "/events", "event_id+changed").unwrap();
        assert_eq!(before.len(), 1, "an item lacking a part is skipped");
        assert_ne!(before, after);
    }

    #[test]
    fn a_poll_sends_its_arguments_with_times_filled_in() {
        let now = DateTime::parse_from_rfc3339("2026-11-10T12:00:00Z").unwrap().with_timezone(&Utc);
        let watch = w("list_events", "/events", "event_id", Some(json!({ "start_time": "{now}", "changed_since": "{now-1d}", "until": "{now+6h}", "query": "{nowhere" })));
        assert_eq!(
            input(&watch, now),
            Some(json!({ "start_time": "2026-11-10T12:00:00Z", "changed_since": "2026-11-09T12:00:00Z", "until": "2026-11-10T18:00:00Z", "query": "{nowhere" }))
        );
        assert_eq!(args(&w("list_inbox", "/messages", "id", None), now), json!({}));
    }

    #[test]
    fn a_watch_is_of_a_read_once_with_its_items_and_id() {
        let class = |t: &str| match t {
            "list" => Some("read".to_string()),
            "send" => Some("irreversible".to_string()),
            _ => None,
        };
        assert_eq!(fault("a", &[w("list", "/items", "id+changed", Some(json!({ "since": "{now-2d}" })))], class), None);
        for bad in [
            vec![w("send", "/items", "id", None)],
            vec![w("gone", "/items", "id", None)],
            vec![w("list", "items", "id", None)],
            vec![w("list", "/items", "", None)],
            vec![w("list", "/items", "id+", None)],
            vec![w("list", "/items", "id", Some(json!(["x"])))],
            vec![w("list", "/items", "id", Some(json!({ "since": "{now-2w}" })))],
            vec![w("list", "/items", "id", None), w("list", "/other", "id", None)],
            vec![Watch { poll_minutes: Some(0), ..w("list", "/items", "id", None) }],
        ] {
            assert!(fault("a", &bad, class).is_some(), "{bad:?}");
        }
    }
}
