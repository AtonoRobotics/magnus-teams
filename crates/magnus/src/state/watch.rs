//! Watched reads (#369, #406): each person's baselines of what their apps'
//! watched tools answered, and when each was last polled on schedule.

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

/// A watched read: person, app and tool.
type Key = (String, String, String);

fn key(person: &str, app: &str, tool: &str) -> Key {
    (person.to_string(), app.to_string(), tool.to_string())
}

#[derive(Default, Debug)]
pub struct Watch {
    /// The last two answers' item ids per person, app and tool, newest
    /// last. New ids wake a review. A window, not an accumulation (#409):
    /// two answers bound the memory whatever the source holds, an
    /// unchanged answer always diffs empty against its own window, and an
    /// item absent for one read stays known while one gone for two wakes
    /// again on return.
    seen: HashMap<Key, Vec<HashSet<String>>>,
    /// When each watched read was last polled on schedule (#406): the next
    /// poll is due an interval after this, or at once when there is none.
    last_polled: HashMap<Key, DateTime<Utc>>,
}

/// Their baselines and poll clocks: whoever is next known by their name
/// starts fresh (#406, #412).
impl crate::forget::Forget for Watch {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.seen.retain(|(p, _, _), _| !who.is(p));
        self.last_polled.retain(|(p, _, _), _| !who.is(p));
    }
}

impl Watch {
    /// A read's answer (`WatchedSeen`). Tagged wholes push as they come
    /// (#415). Untagged history splits by overlap (#423): era-1 deltas are
    /// fresh-only, hence always disjoint from the window, and union onto
    /// the newest entry they extend; an overlapping untagged event is an
    /// era-2 whole answer and pushes. A total-churn era-2 pair is
    /// indistinguishable from seed plus delta and unions transiently,
    /// healed by later tagged reads.
    pub(crate) fn fold_seen(&mut self, person: &str, app: &str, tool: &str, ids: &[String], full: bool) {
        let window = self.seen.entry(key(person, app, tool)).or_default();
        let seen: HashSet<String> = ids.iter().cloned().collect();
        let entry = if full || window.iter().flatten().any(|id| seen.contains(id)) {
            seen
        } else {
            let mut union = window.last().cloned().unwrap_or_default();
            union.extend(seen);
            union
        };
        window.push(entry);
        if window.len() > 2 {
            window.remove(0);
        }
    }

    /// A scheduled poll started (`WatchPolled`).
    pub(crate) fn fold_polled(&mut self, person: &str, app: &str, tool: &str, at: DateTime<Utc>) {
        self.last_polled.insert(key(person, app, tool), at);
    }

    /// The read's last two answers, oldest first; None before its first.
    pub fn window(&self, person: &str, app: &str, tool: &str) -> Option<&Vec<HashSet<String>>> {
        self.seen.get(&key(person, app, tool))
    }

    /// When the read was last polled on schedule.
    pub fn last_polled(&self, person: &str, app: &str, tool: &str) -> Option<DateTime<Utc>> {
        self.last_polled.get(&key(person, app, tool)).copied()
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![("watched_seen", format!("{:?}", self.seen)), ("watch_last_polled", format!("{:?}", self.last_polled))]
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.seen.is_empty() && self.last_polled.is_empty()
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
