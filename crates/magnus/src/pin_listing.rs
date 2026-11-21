//! The listing the boundary compares with a tool's pin before acting
//! (`Kernel::check_pin`; decisions/2026-11-19-pin-relist-interval.md).
//! Listing an app's tools before every call costs a network round trip per
//! action and denies the action when the listing fails. A run reuses its
//! own listing of an app for `PIN_RELIST_INTERVAL`, and only while the
//! listed definition is the one pinned: a listing that lacks the tool, or
//! whose definition differs from the pin, is never reused, and the app is
//! listed again. A new run always lists afresh, so a definition changed
//! between tasks is caught before the next task acts with it. The reused
//! definition still goes through the same pin and screen as a fresh one.

use crate::apps::{definition_hash, AppTool};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long a run acts on its own listing of an app before listing it again.
pub const PIN_RELIST_INTERVAL: Duration = Duration::from_secs(60);

/// App, person, run.
type Key = (String, String, String);

/// Each run's latest listing of each app it acted in, and when it was made.
#[derive(Default)]
pub struct Listings {
    at: HashMap<Key, (Instant, Vec<AppTool>)>,
}

fn key(app: &str, person: &str, run: &str) -> Key {
    (app.to_string(), person.to_string(), run.to_string())
}

impl Listings {
    /// `tool`'s definition from this run's listing of `app`, when that is
    /// younger than the interval at `now` and its definition hashes to
    /// `pinned`. None: list the app again. Outside a run, never reused.
    pub fn reuse(&self, app: &str, person: &str, run: &str, tool: &str, pinned: Option<&str>, now: Instant) -> Option<AppTool> {
        if run.is_empty() {
            return None;
        }
        let (at, tools) = self.at.get(&key(app, person, run))?;
        if now.saturating_duration_since(*at) >= PIN_RELIST_INTERVAL {
            return None;
        }
        let t = tools.iter().find(|t| t.name == tool)?;
        (pinned == Some(definition_hash(t).as_str())).then(|| t.clone())
    }

    /// Keep a fresh listing for the run; listings past the interval go.
    pub fn keep(&mut self, app: &str, person: &str, run: &str, tools: &[AppTool], now: Instant) {
        self.at.retain(|_, (at, _)| now.saturating_duration_since(*at) < PIN_RELIST_INTERVAL);
        if !run.is_empty() {
            self.at.insert(key(app, person, run), (now, tools.to_vec()));
        }
    }
}

static LISTINGS: LazyLock<Mutex<Listings>> = LazyLock::new(Mutex::default);

/// `Listings::reuse`, now, from the server's listings.
pub fn reuse(app: &str, person: &str, run: &str, tool: &str, pinned: Option<&str>) -> Option<AppTool> {
    LISTINGS.lock().unwrap_or_else(|e| e.into_inner()).reuse(app, person, run, tool, pinned, Instant::now())
}

/// `Listings::keep`, now, in the server's listings.
pub fn keep(app: &str, person: &str, run: &str, tools: &[AppTool]) {
    LISTINGS.lock().unwrap_or_else(|e| e.into_inner()).keep(app, person, run, tools, Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, description: &str) -> AppTool {
        AppTool { name: name.into(), description: description.into(), input_schema: json!({}), annotations: json!({}) }
    }

    fn listed(now: Instant) -> (Listings, String) {
        let t = tool("send", "Send.");
        let pinned = definition_hash(&t);
        let mut l = Listings::default();
        l.keep("crm", "sam", "run1", &[t], now);
        (l, pinned)
    }

    #[test]
    fn a_runs_listing_is_reused_within_the_interval_while_it_matches_the_pin() {
        let now = Instant::now();
        let (l, pinned) = listed(now);
        let later = now + PIN_RELIST_INTERVAL - Duration::from_secs(1);
        assert_eq!(l.reuse("crm", "sam", "run1", "send", Some(&pinned), later).map(|t| t.description), Some("Send.".into()));
    }

    #[test]
    fn a_listing_past_the_interval_is_listed_again() {
        let now = Instant::now();
        let (l, pinned) = listed(now);
        assert!(l.reuse("crm", "sam", "run1", "send", Some(&pinned), now + PIN_RELIST_INTERVAL).is_none(), "a listing a minute old was reused");
    }

    #[test]
    fn a_listing_that_lacks_the_tool_or_differs_from_the_pin_is_listed_again() {
        let now = Instant::now();
        let (l, pinned) = listed(now);
        assert!(l.reuse("crm", "sam", "run1", "other", Some(&pinned), now).is_none(), "a listing without the tool was reused");
        assert!(l.reuse("crm", "sam", "run1", "send", Some("another hash"), now).is_none(), "a listing that differs from the pin was reused");
        assert!(l.reuse("crm", "sam", "run1", "send", None, now).is_none(), "a listing of a tool with no usable pin was reused");
    }

    #[test]
    fn a_listing_serves_only_its_own_run_person_and_app() {
        let now = Instant::now();
        let (l, pinned) = listed(now);
        assert!(l.reuse("crm", "sam", "run2", "send", Some(&pinned), now).is_none(), "a new run reused an earlier run's listing");
        assert!(l.reuse("crm", "kim", "run1", "send", Some(&pinned), now).is_none(), "one person's listing served another");
        assert!(l.reuse("mail", "sam", "run1", "send", Some(&pinned), now).is_none(), "one app's listing served another");
        let mut outside = Listings::default();
        outside.keep("crm", "sam", "", &[tool("send", "Send.")], now);
        assert!(outside.reuse("crm", "sam", "", "send", Some(&pinned), now).is_none(), "a listing outside a run was reused");
    }

    #[test]
    fn stale_listings_are_put_away() {
        let now = Instant::now();
        let (mut l, _) = listed(now);
        l.keep("mail", "sam", "run1", &[], now + PIN_RELIST_INTERVAL);
        assert_eq!(l.at.len(), 1, "a stale listing was kept");
    }
}
