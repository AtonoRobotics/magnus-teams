//! Proactive schedule windows, quiet hours, and wake reason helpers.

use crate::cadence::{zone_of, Zone};
use crate::tools::Label;
use chrono::{DateTime, Timelike, Utc};

/// Open commitments due within this many hours — or already overdue — trigger a proactive wake.
const COMMITMENT_WAKE_HORIZON_HOURS: i64 = 24;

/// Whether a commitment due at `due` should wake the proactive loop at `now`.
/// Includes already-overdue commitments (`due <= now`) as well as those due within the horizon.
pub(crate) fn commitment_due_for_wake(due: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    due <= now + chrono::Duration::hours(COMMITMENT_WAKE_HORIZON_HOURS)
}

/// The ended task that woke a review: its ID, its outcome line, and the
/// taint it ended with, which the review starts with (#383, #390).
pub(crate) struct Ended {
    pub task: String,
    pub outcome: String,
    pub label: Label,
}

/// Minutes since midnight for a "HH:MM" time, in the person's timezone.
fn at(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then(|| h * 60 + m)
}

fn window(v: &serde_json::Value, fallback: (u32, u32)) -> (u32, u32) {
    let s = v.get("start").and_then(serde_json::Value::as_str).and_then(at);
    let e = v.get("end").and_then(serde_json::Value::as_str).and_then(at);
    match (s, e) {
        (Some(s), Some(e)) => (s, e),
        _ => fallback,
    }
}

/// The person's resolved loop settings.
pub struct Settings {
    /// Their zone, as their schedules read it (`cadence::zone_of`).
    pub tz: Zone,
    pub working: (u32, u32),
    pub quiet: (u32, u32),
    pub every_minutes: u32,
    pub per_day: u32,
}

/// Resolve settings: preferences over the `[proactive]` config, defaults
/// where either is missing or unparsable. Working 9 to 5, quiet 10pm to
/// 7am, weekly cadence floor of 5 minutes.
pub fn settings(cfg: &crate::config::Proactive, prefs: &serde_json::Value) -> Settings {
    let num = |k: &str, dflt: u32| prefs.get(k).and_then(serde_json::Value::as_u64).map(|n| n as u32).filter(|n| *n > 0).unwrap_or(dflt);
    Settings {
        tz: zone_of(Some(prefs)),
        working: prefs.get("working_hours").map(|w| window(w, (540, 1020))).unwrap_or((540, 1020)),
        quiet: prefs.get("quiet_hours").map(|w| window(w, (1320, 420))).unwrap_or((1320, 420)),
        every_minutes: num("review_every_minutes", cfg.review_every_minutes).max(5),
        per_day: num("interruptions_per_day", cfg.interruptions_per_day),
    }
}

/// Minutes since midnight where the person is.
pub fn minutes_at(now: DateTime<Utc>, tz: Zone) -> u32 {
    let local = tz.local(now);
    local.hour() * 60 + local.minute()
}

/// Whether minute-of-day `m` falls in the window, which may cross midnight
/// (an end at or before its start runs into the next day).
pub fn in_window(m: u32, (start, end): (u32, u32)) -> bool {
    if end > start {
        m >= start && m < end
    } else {
        m >= start || m < end
    }
}

/// The person's calendar day, for the daily interruption budget.
pub fn day_key(now: DateTime<Utc>, tz: Zone) -> String {
    tz.local(now).date().to_string()
}

/// When the current quiet hours end, for delaying an interruption's
/// notification until then. None when it is not quiet now.
pub fn quiet_ends_at(now: DateTime<Utc>, tz: Zone, quiet: (u32, u32)) -> Option<DateTime<Utc>> {
    if !in_window(minutes_at(now, tz), quiet) {
        return None;
    }
    let local = tz.local(now);
    let mut day = local.date();
    if quiet.1 <= quiet.0 && local.hour() * 60 + local.minute() >= quiet.0 {
        day = day.succ_opt()?;
    }
    let naive = day.and_hms_opt(quiet.1 / 60, quiet.1 % 60, 0)?;
    tz.earliest(&naive)
}

/// The window's next start, for holding an interruption's notification
/// until working hours. None when in the window now.
pub fn window_next_start(now: DateTime<Utc>, tz: Zone, window: (u32, u32)) -> Option<DateTime<Utc>> {
    if in_window(minutes_at(now, tz), window) {
        return None;
    }
    let local = tz.local(now);
    let mut day = local.date();
    if window.1 > window.0 && local.hour() * 60 + local.minute() >= window.0 {
        day = day.succ_opt()?;
    }
    let naive = day.and_hms_opt(window.0 / 60, window.0 % 60, 0)?;
    tz.earliest(&naive)
}

/// A coming-up schedule's wake reason, carrying its id for the once-a-day
/// mark the log keeps (#237).
pub fn schedule_reason(id: &str) -> String {
    format!("schedule {id} coming up")
}

/// A due commitment's wake reason, carrying its id likewise.
pub fn commitment_reason(id: &str) -> String {
    format!("commitment {id} due")
}

/// The person's open commitment a review wakes for now: due within the
/// next day (or already overdue) and not yet woken for at that due time.
/// Marked by when it is due, not by day, so a day before it that crosses
/// the person's midnight wakes one review, not two.
pub(crate) fn commitment_to_wake(s: &crate::state::State, person: &str, tz: Zone, now: DateTime<Utc>) -> Option<String> {
    s.arrangements
        .commitments()
        .values()
        .filter(|c| c.person == person && c.status == "open")
        .find(|c| {
            c.by.as_deref().and_then(|b| crate::commitments::due(b, tz)).is_some_and(|d| commitment_due_for_wake(d, now))
                && s.inbox.wake_marks().get(&crate::state::inbox::commitment_mark(person, &c.id)).map(String::as_str) != c.by.as_deref()
        })
        .map(|c| c.id.clone())
}

/// A stale approval's wake reason, carrying its item likewise.
pub fn approval_reason(id: &str) -> String {
    format!("approval {id} waiting")
}
