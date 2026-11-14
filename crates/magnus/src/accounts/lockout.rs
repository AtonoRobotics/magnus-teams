//! The lockout rule, without a clock or a store: wrong passwords per
//! counter, and until when a counter stays locked. The caller passes the
//! time and the sign-in rules, so the rule is tested at fixed instants.
//! The counters live in memory, as before: a restart forgives guesses in
//! flight. Counters arrive canonical (the caller lowercases them).

use super::Policy;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;

/// How long a permanent lockout is recorded for: nobody's lock outlives
/// the century, and an admin's enable clears it anyway.
pub(super) const PERMANENT_YEARS: i64 = 100;

/// Wrong passwords on one counter: how many, since when, and until when
/// the counter stays locked, if it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Failures {
    pub(super) n: u32,
    pub(super) first: DateTime<Utc>,
    pub(super) until: Option<DateTime<Utc>>,
}

/// Every counter's failures.
#[derive(Default, Debug)]
pub(super) struct Lockout {
    failures: BTreeMap<String, Failures>,
}

impl Lockout {
    /// Until when `counter` may not try another password at `now`. A
    /// lapsed lock, or failures gone quiet past the longest wait, forgives
    /// the counter. With the lockout off, nothing is locked.
    pub(super) fn locked(&mut self, counter: &str, now: DateTime<Utc>, p: &Policy) -> Option<DateTime<Utc>> {
        if !p.lockout_enabled {
            return None;
        }
        let horizon = Duration::minutes(p.lockout_max_wait_minutes.max(1));
        let r = self.failures.get(counter).copied()?;
        if r.until.is_some_and(|u| now < u) {
            return r.until;
        }
        if r.until.is_some() || now - r.first > horizon {
            self.failures.remove(counter);
        }
        None
    }

    /// Count a wrong password at `now`. Past the threshold each failure
    /// holds the counter until now plus the first wait (or the live lock
    /// plus it), capped at the first failure plus the longest wait — or
    /// out the century, when a lockout is permanent. With the lockout
    /// off, nothing is counted.
    pub(super) fn failed(&mut self, counter: &str, now: DateTime<Utc>, p: &Policy) {
        if !p.lockout_enabled {
            return;
        }
        let e = self.failures.entry(counter.to_string()).or_insert(Failures { n: 0, first: now, until: None });
        e.n += 1;
        if e.n >= p.lockout_failures.max(1) as u32 {
            e.until = Some(if p.lockout_permanent {
                e.first + Duration::days(365 * PERMANENT_YEARS)
            } else {
                (e.until.filter(|u| *u > now).unwrap_or(now) + Duration::minutes(p.lockout_wait_minutes.max(1))).min(e.first + Duration::minutes(p.lockout_max_wait_minutes.max(1)))
            });
        }
    }

    /// A right password: the counter starts over.
    pub(super) fn succeeded(&mut self, counter: &str) {
        self.failures.remove(counter);
    }

    /// Take back one counted attempt, whose answer said nothing about the
    /// password.
    pub(super) fn forgive(&mut self, counter: &str) {
        if let Some(e) = self.failures.get_mut(counter) {
            e.n = e.n.saturating_sub(1);
        }
    }

    /// Forget `counter` whole, locked or not.
    pub(super) fn clear(&mut self, counter: &str) {
        self.failures.remove(counter);
    }

    /// How many failures `counter` holds.
    #[cfg(test)]
    pub(super) fn count(&self, counter: &str) -> u32 {
        self.failures.get(counter).map_or(0, |f| f.n)
    }

    /// Forget every counter.
    #[cfg(test)]
    pub(super) fn clear_all(&mut self) {
        self.failures.clear();
    }

    /// Move a counter's lock to end at `until`, to test its lapse.
    #[cfg(test)]
    pub(super) fn set_until(&mut self, counter: &str, until: DateTime<Utc>) {
        if let Some(e) = self.failures.get_mut(counter) {
            e.until = Some(until);
        }
    }

    #[cfg(test)]
    fn get(&self, counter: &str) -> Option<Failures> {
        self.failures.get(counter).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "net:ada";

    fn rules(failures: i64, wait: i64, max_wait: i64) -> Policy {
        Policy {
            code_required: false,
            idle_minutes: 30,
            max_hours: 24,
            lockout_enabled: true,
            lockout_failures: failures,
            lockout_wait_minutes: wait,
            lockout_max_wait_minutes: max_wait,
            lockout_permanent: false,
        }
    }

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn mins(m: i64) -> Duration {
        Duration::minutes(m)
    }

    #[test]
    fn the_threshold_locks_for_the_first_wait() {
        let p = rules(3, 5, 15);
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        l.failed(C, t0() + mins(1), &p);
        assert_eq!(l.locked(C, t0() + mins(1), &p), None, "two of three locked");
        l.failed(C, t0() + mins(2), &p);
        assert_eq!(l.locked(C, t0() + mins(2), &p), Some(t0() + mins(7)), "the third did not lock for the first wait");
        assert_eq!(l.locked(C, t0() + mins(6), &p), Some(t0() + mins(7)), "the lock ended early");
    }

    #[test]
    fn failing_while_locked_extends_up_to_the_longest_wait_from_the_first() {
        let p = rules(2, 5, 12);
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        l.failed(C, t0(), &p);
        assert_eq!(l.locked(C, t0(), &p), Some(t0() + mins(5)));
        // A locked attempt counts: the lock grows from its own end.
        l.failed(C, t0() + mins(1), &p);
        assert_eq!(l.locked(C, t0() + mins(1), &p), Some(t0() + mins(10)), "a locked attempt did not extend from the lock's end");
        l.failed(C, t0() + mins(2), &p);
        assert_eq!(l.locked(C, t0() + mins(2), &p), Some(t0() + mins(12)), "the lock passed the first failure plus the longest wait");
    }

    #[test]
    fn a_lapsed_lock_forgives_the_counter_and_waiting_never_extends() {
        let p = rules(2, 5, 15);
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        l.failed(C, t0(), &p);
        assert_eq!(l.locked(C, t0() + mins(5), &p), None, "the lock held at its end");
        assert_eq!(l.count(C), 0, "the lapsed lock kept its count");
        l.failed(C, t0() + mins(6), &p);
        assert_eq!(l.locked(C, t0() + mins(6), &p), None, "one failure after a lapse locked again");
    }

    #[test]
    fn quiet_failures_are_forgiven_past_the_longest_wait_only() {
        let p = rules(3, 5, 15);
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        assert_eq!(l.locked(C, t0() + mins(15), &p), None);
        assert_eq!(l.count(C), 1, "failures inside the longest wait were forgiven");
        assert_eq!(l.locked(C, t0() + mins(15) + Duration::seconds(1), &p), None);
        assert_eq!(l.count(C), 0, "failures quiet past the longest wait were kept");
    }

    #[test]
    fn a_permanent_lock_holds_out_the_century_from_the_first_failure() {
        let mut p = rules(1, 5, 15);
        p.lockout_permanent = true;
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        let until = t0() + Duration::days(365 * PERMANENT_YEARS);
        assert_eq!(l.locked(C, t0() + Duration::days(365 * 50), &p), Some(until), "the permanent lock ended");
        l.clear(C);
        assert_eq!(l.locked(C, t0(), &p), None, "clearing left the permanent lock");
    }

    #[test]
    fn with_the_lockout_off_nothing_counts_or_locks() {
        let on = rules(1, 5, 15);
        let mut off = on;
        off.lockout_enabled = false;
        let mut l = Lockout::default();
        l.failed(C, t0(), &off);
        assert_eq!(l.count(C), 0, "a failure counted with the lockout off");
        l.failed(C, t0(), &on);
        assert_eq!(l.locked(C, t0(), &off), None, "a lock held with the lockout off");
        assert!(l.get(C).is_some(), "turning the lockout off erased the counter");
    }

    #[test]
    fn forgive_takes_back_one_and_success_starts_over() {
        let p = rules(3, 5, 15);
        let mut l = Lockout::default();
        l.forgive(C);
        assert_eq!(l.count(C), 0, "forgiving nothing made a counter");
        l.failed(C, t0(), &p);
        l.failed(C, t0(), &p);
        l.forgive(C);
        assert_eq!(l.count(C), 1);
        l.forgive(C);
        l.forgive(C);
        assert_eq!(l.count(C), 0, "forgiving went below zero");
        l.failed(C, t0(), &p);
        l.succeeded(C);
        assert!(l.get(C).is_none(), "success kept the counter");
    }

    #[test]
    fn counters_are_apart_and_zero_rules_mean_one() {
        let p = rules(0, 0, 0);
        let mut l = Lockout::default();
        l.failed(C, t0(), &p);
        assert_eq!(l.locked(C, t0(), &p), Some(t0() + mins(1)), "zero failures or minutes did not read as one");
        assert_eq!(l.locked("local:ada", t0(), &p), None, "one counter's lock held another");
    }
}
