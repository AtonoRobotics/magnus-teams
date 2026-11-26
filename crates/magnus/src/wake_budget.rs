//! A person's budget of background wake-ups
//! (decisions/2026-11-23-a-limit-on-background-wake-ups.md): at most
//! `MAX_WAKES_PER_HOUR` reviews in any hour and `MAX_WAKES_PER_DAY` in any
//! day, so no one outside (a stranger mailing a watched mailbox once a
//! minute) can flood their reviews or spend their month. Every review
//! starts through `start_review` (`proactive.rs`), which asks here first.
//! The wakes are counted from the log (`ReviewWoken`, folded in
//! `state::inbox`), so a restart forgets none.

use crate::state::State;
use chrono::{DateTime, Duration, Utc};

/// Reviews a person may have woken in any hour.
pub const MAX_WAKES_PER_HOUR: usize = 10;
/// Reviews a person may have woken in any day.
pub const MAX_WAKES_PER_DAY: usize = 50;
/// The hour they count in.
pub const HOUR_SECS: i64 = 3_600;
/// The day they count in, and how long the log's wakes are kept for it.
pub const DAY_SECS: i64 = 86_400;

/// Why no review may wake for `person` at `now`, when none may: the wakes
/// of the last hour or day are spent.
pub fn refused(s: &State, person: &str, now: DateTime<Utc>) -> Option<String> {
    let since = |secs| s.inbox.wakes_of(person).filter(|t| **t > now - Duration::seconds(secs)).count();
    if since(DAY_SECS) >= MAX_WAKES_PER_DAY {
        return Some(format!("{MAX_WAKES_PER_DAY} reviews woke in the last day, the most a day allows"));
    }
    if since(HOUR_SECS) >= MAX_WAKES_PER_HOUR {
        return Some(format!("{MAX_WAKES_PER_HOUR} reviews woke in the last hour, the most an hour allows"));
    }
    None
}

/// How long new items in a person's apps gather after a review of new
/// items woke before the next may (owner, 2026-11-25: one review per new
/// email was the largest spender, 94M of a week's 215M input tokens). The
/// items are not lost: a wake turned away advances no baseline, so the
/// next read after the window wakes one review for all of them.
pub const ARRIVAL_GATHER_MINUTES: u32 = 10;

/// Whether new items for `person` still gather at `now`: a review of new
/// items woke less than `minutes` ago.
pub fn gathering(s: &State, person: &str, now: DateTime<Utc>, minutes: u32) -> bool {
    s.inbox.arrival_review_of(person).is_some_and(|at| now < at + Duration::minutes(i64::from(minutes)))
}

#[cfg(test)]
#[path = "wake_budget_tests.rs"]
mod tests;
