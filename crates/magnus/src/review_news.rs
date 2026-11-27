//! What a review has that is news. In production on 2026-11-27 the
//! reviews queued 57 observations in a day, almost all the same news in
//! other words: two schedules paused after failing. Every review read the
//! same state and nothing showed it what earlier reviews had said.
//!
//! - A review is shown what it already told the person.
//! - `observe` and `interrupt` refuse news already told: an item of the last
//!   `ALREADY_TOLD_DAYS` names the same things (its ids) or says the same.
//! - A cadence review starts only when the state it reads has changed since
//!   the last review started (`Kernel::state_unchanged`).

use crate::kernel::Kernel;
use crate::state::{Item, State};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// How far back what was told counts as told.
pub const ALREADY_TOLD_DAYS: i64 = 7;
/// The shortest id that is matched as a prefix of another: a model
/// sometimes drops an id's last characters.
const MIN_ID_PREFIX_CHARS: usize = 16;
const ID_PREFIXES: [&str; 4] = ["sched_", "task_", "item_", "run_"];

/// The observations and interruptions `person` was given in the last
/// `ALREADY_TOLD_DAYS`, newest first, read or not, dismissed or not.
pub(crate) fn told<'a>(s: &'a State, person: &str, now: DateTime<Utc>) -> Vec<&'a Item> {
    let since = now - chrono::Duration::days(ALREADY_TOLD_DAYS);
    let mut items: Vec<&Item> = s.inbox.items().values().filter(|i| i.person == person && (i.kind == "observation" || i.kind == "interrupt") && i.created_at >= since).collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.created_at));
    items
}

/// The ids of Magnus's own things that `text` names: schedules, tasks,
/// items and runs.
fn ids(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| ID_PREFIXES.iter().any(|p| w.starts_with(p) && w.len() > p.len() + 8)).map(str::to_string).collect()
}

fn same_id(a: &str, b: &str) -> bool {
    a == b || (a.len().min(b.len()) >= MIN_ID_PREFIX_CHARS && (a.starts_with(b) || b.starts_with(a)))
}

/// The item that already told `person` this news, if one did: everything
/// the new one names was named by what was told, or one of them says the
/// same.
pub(crate) fn repeat_of(s: &State, person: &str, what: &str, why: &str, evidence: &str, now: DateTime<Utc>) -> Option<String> {
    let told = told(s, person, now);
    if let Some(i) = told.iter().find(|i| crate::memory_index::same_fact(what, &i.summary)) {
        return Some(i.id.clone());
    }
    let named = ids(&format!("{what} {why} {evidence}"));
    if named.is_empty() {
        return None;
    }
    let mut covered = BTreeSet::new();
    for i in &told {
        let before = ids(&format!("{} {}", i.summary, i.details));
        covered.extend(before);
        if named.iter().all(|n| covered.iter().any(|c| same_id(n, c))) {
            return Some(i.id.clone());
        }
    }
    None
}

/// The review's list of what it already told, one line each.
pub(crate) fn told_lines(s: &State, person: &str, now: DateTime<Utc>) -> Vec<String> {
    told(s, person, now)
        .iter()
        .map(|i| {
            format!(
                "{} ({}): {}",
                i.created_at.format("%b %-d %H:%M UTC"),
                if i.resolution.is_some() { "dismissed" } else { "open" },
                i.summary.chars().take(200).collect::<String>()
            )
        })
        .collect()
}

/// A fingerprint of what a review reads about `person`: their unfinished
/// work, schedules, waiting approvals, open disagreements and open
/// commitments. What reviews told them is not part of it, so a review's own
/// observation never makes the next one run.
pub(crate) fn state_print(s: &State, person: &str, now: DateTime<Utc>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut tasks: Vec<_> = s
        .tasks
        .values()
        .filter(|t| t.person == person && !t.review && !t.poll)
        .filter_map(|t| s.runs.get(&t.run_id).filter(|r| !r.status.terminal()).map(|r| (t.task_id.clone(), format!("{:?}", r.status), r.reason.clone())))
        .collect();
    tasks.sort();
    tasks.hash(&mut h);
    let mut schedules: Vec<_> = s
        .arrangements
        .schedules()
        .values()
        .filter(|sc| sc.person == person)
        .map(|sc| (sc.id.clone(), sc.paused.clone(), sc.failures.len(), sc.next_due().map(|d| d.timestamp())))
        .collect();
    schedules.sort();
    schedules.hash(&mut h);
    let mut approvals: Vec<_> = s.inbox.items().values().filter(|i| i.person == person && i.kind == "approval" && i.resolution.is_none()).map(|i| i.id.clone()).collect();
    approvals.sort();
    approvals.hash(&mut h);
    let mut disagreements: Vec<_> = s.thread_extras.disagreements().iter().filter(|(_, d)| d.person == person && d.outcome.is_none()).map(|(id, _)| id.clone()).collect();
    disagreements.sort();
    disagreements.hash(&mut h);
    let zone = s.identity.zone(person);
    let mut commitments: Vec<_> = s
        .arrangements
        .commitments()
        .values()
        .filter(|c| c.person == person && c.status == "open")
        .map(|c| (c.id.clone(), c.by.as_deref().and_then(|b| crate::commitments::due(b, zone)).is_some_and(|d| d < now)))
        .collect();
    commitments.sort();
    commitments.hash(&mut h);
    h.finish()
}

impl Kernel {
    /// Whether what a review of `person` reads is as it was when their last
    /// review started. A server just started knows no last review.
    pub(crate) fn state_unchanged(&self, person: &str, now: DateTime<Utc>) -> bool {
        let print = self.read(|s| state_print(s, person, now));
        self.reviewed_state.lock().is_ok_and(|m| m.get(person) == Some(&print))
    }

    /// A review of `person` starts over the state as it is now.
    pub(crate) fn note_reviewed_state(&self, person: &str, now: DateTime<Utc>) {
        let print = self.read(|s| state_print(s, person, now));
        if let Ok(mut m) = self.reviewed_state.lock() {
            m.insert(person.to_string(), print);
        }
    }

    /// The refusal for news `person` was already given, if it was.
    pub(crate) fn already_told(&self, person: &str, what: &str, why: &str, evidence: &str) -> Option<String> {
        let id = self.read(|s| repeat_of(s, person, what, why, evidence, Utc::now()))?;
        Some(format!("Already told: {id} gave the person this news, and nothing was queued. Raise it again only with something it did not say, naming what changed."))
    }
}

#[cfg(test)]
#[path = "review_news_tests.rs"]
mod tests;
