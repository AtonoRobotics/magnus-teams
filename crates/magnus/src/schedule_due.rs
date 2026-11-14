//! When a person's schedule may fire: its limits and its next time, the
//! arithmetic shared with routines (`cadence`), its daily time read in its
//! person's zone.
use crate::state::Schedule;
use chrono::{DateTime, Utc};

impl Schedule {
    /// Whether it may start no more runs: past its end, or out of runs.
    pub fn finished(&self, now: DateTime<Utc>) -> Option<String> {
        if self.ended.is_some() {
            return None;
        }
        if let Some(m) = self.max_runs.filter(|m| self.started >= *m) {
            return Some(format!("it ran {m} time{}, as set", if m == 1 { "" } else { "s" }));
        }
        self.until.filter(|u| *u <= now).map(|u| format!("it reached its end, {}", u.to_rfc3339()))
    }

    /// The next time this schedule is due after its last run (or creation),
    /// unless it is paused, ended (by its own run or a limit) or out of runs.
    pub fn next_due(&self) -> Option<DateTime<Utc>> {
        self.next_due_in(self.zone)
    }

    /// `next_due`, its daily time read in `zone`.
    pub fn next_due_in(&self, zone: crate::cadence::Zone) -> Option<DateTime<Utc>> {
        if self.ended.is_some() || self.paused.is_some() || self.max_runs.is_some_and(|m| self.started >= m) {
            return None;
        }
        let next = self.next_time(zone)?;
        self.until.is_none_or(|u| next < u).then_some(next)
    }

    fn next_time(&self, zone: crate::cadence::Zone) -> Option<DateTime<Utc>> {
        let every = crate::cadence::Recurrence::of_schedule(self.every_minutes, self.daily_at.as_deref())?;
        every.next_after(self.last_fired, self.created, zone)
    }
}
