//! When a recurring thing is next due: the arithmetic a person's schedules
//! and a team's routines share, and nothing else. What firing means, whose
//! authority and budget it spends, and what it records stay with each
//! (`state::Schedule`, `team::routines`). Pure: the clock and the time zone
//! are passed in.
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc, Weekday};

/// Bitmask for Monday through Friday (`Mon=1 << 0` .. `Sun=1 << 6`).
const WEEKDAYS_MASK: u8 = 0b001_1111;
/// Maximum days scanned forward when finding the next matching calendar date.
const MAX_CALENDAR_SCAN_DAYS: usize = 370;

/// How often something recurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recurrence {
    /// Every so many minutes (at least one).
    EveryMinutes(u32),
    /// Every so many hours.
    EveryHours(i64),
    /// Once a day at `h:m`, wall-clock time in the zone it is read in.
    DailyAt { h: u32, m: u32 },
    /// Monday through Friday at `h:m`, wall-clock time.
    WeekdaysAt { h: u32, m: u32 },
    /// Specific weekdays (`bit 0 = Mon` .. `bit 6 = Sun`) at `h:m`, wall-clock time.
    WeeklyAt { mask: u8, h: u32, m: u32 },
    /// A specific day of the month (`1..=31`, clamped to the month's last day) at `h:m`.
    MonthlyAt { day: u32, h: u32, m: u32 },
    /// A single wall-clock time (or UTC instant) that runs once and does not repeat.
    OnceAt { at: NaiveDateTime, utc: bool },
}

/// The time zone a daily time is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// A named zone (a person's own).
    Named(chrono_tz::Tz),
    /// The server's own clock.
    Server,
}

/// A person's zone from their preferences (`timezone`, an IANA name); the
/// server's when they set none, or one that is not a zone. The one reading
/// of a person's zone: their schedules, working and quiet hours, the day
/// their interruptions and reviews count in, Settings and what the model is
/// told all go through it (owner, 2026-11-14: the server's, not UTC).
pub fn zone_of(prefs: Option<&serde_json::Value>) -> Zone {
    prefs.and_then(|p| p.get("timezone")).and_then(serde_json::Value::as_str).and_then(|z| z.parse().ok()).map_or(Zone::Server, Zone::Named)
}

impl Zone {
    /// The wall-clock time here at `t`.
    pub fn local(&self, t: DateTime<Utc>) -> chrono::NaiveDateTime {
        match self {
            Zone::Named(tz) => t.with_timezone(tz).naive_local(),
            Zone::Server => t.with_timezone(&chrono::Local).naive_local(),
        }
    }

    /// The earliest moment the clocks here show `wall`; none when they skip it.
    pub fn earliest(&self, wall: &chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
        use chrono::TimeZone;
        match self {
            Zone::Named(tz) => tz.from_local_datetime(wall).earliest().map(|t| t.with_timezone(&Utc)),
            Zone::Server => chrono::Local.from_local_datetime(wall).earliest().map(|t| t.with_timezone(&Utc)),
        }
    }

    /// Its IANA name: the person's, or the server's (`server`, read with the
    /// config at start).
    pub fn name(&self, server: &str) -> String {
        match self {
            Zone::Named(tz) => tz.name().to_string(),
            Zone::Server => server.to_string(),
        }
    }
}

fn parse_hm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

fn weekday_bit(w: Weekday) -> u8 {
    1u8 << w.num_days_from_monday()
}

fn parse_weekday_token(tok: &str) -> Option<u8> {
    match tok.trim().to_ascii_lowercase().as_str() {
        "mon" | "monday" | "1" => Some(1 << 0),
        "tue" | "tues" | "tuesday" | "2" => Some(1 << 1),
        "wed" | "wednesday" | "3" => Some(1 << 2),
        "thu" | "thur" | "thurs" | "thursday" | "4" => Some(1 << 3),
        "fri" | "friday" | "5" => Some(1 << 4),
        "sat" | "saturday" | "6" => Some(1 << 5),
        "sun" | "sunday" | "0" | "7" => Some(1 << 6),
        _ => None,
    }
}

fn parse_weekday_mask(list: &str) -> Option<u8> {
    let mut mask = 0u8;
    for part in list.split(',') {
        mask |= parse_weekday_token(part)?;
    }
    (mask != 0).then_some(mask)
}

fn canonical_days(mask: u8) -> String {
    const NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    NAMES.iter().enumerate().filter(|(i, _)| (mask & (1 << i)) != 0).map(|(_, n)| *n).collect::<Vec<_>>().join(",")
}

fn parse_once_spec(raw: &str) -> Option<(NaiveDateTime, bool)> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some((dt.with_timezone(&Utc).naive_utc(), true));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some((ndt, false));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Some((ndt, false));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some((ndt, false));
    }
    None
}

fn parse_wall_spec(raw: &str) -> Option<Recurrence> {
    let s = raw.trim();
    if let Some((h, m)) = parse_hm(s) {
        return Some(Recurrence::DailyAt { h, m });
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("weekdays ") {
        let (h, m) = parse_hm(rest)?;
        return Some(Recurrence::WeekdaysAt { h, m });
    }
    if let Some(rest) = lower.strip_prefix("weekly ") {
        let (days, hm) = rest.trim().split_once(char::is_whitespace)?;
        let mask = parse_weekday_mask(days)?;
        let (h, m) = parse_hm(hm)?;
        return Some(Recurrence::WeeklyAt { mask, h, m });
    }
    if let Some(rest) = lower.strip_prefix("monthly ") {
        let (day_str, hm) = rest.trim().split_once(char::is_whitespace)?;
        let day: u32 = day_str.trim().parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        let (h, m) = parse_hm(hm)?;
        return Some(Recurrence::MonthlyAt { day, h, m });
    }
    if let Some(rest) = s.strip_prefix("once ").or_else(|| s.strip_prefix("ONCE ")) {
        let (at, utc) = parse_once_spec(rest)?;
        return Some(Recurrence::OnceAt { at, utc });
    }
    None
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    next_month.and_then(|d| d.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

impl Recurrence {
    /// A schedule's recurrence as it is recorded: `every_minutes` wins over
    /// `daily_at` ("HH:MM", "weekdays HH:MM", "weekly mon,fri HH:MM",
    /// "monthly 15 HH:MM", or "once YYYY-MM-DDTHH:MM"); neither, or a time
    /// that does not parse, is none.
    pub fn of_schedule(every_minutes: Option<u32>, daily_at: Option<&str>) -> Option<Recurrence> {
        if let Some(m) = every_minutes {
            return Some(Recurrence::EveryMinutes(m.max(1)));
        }
        parse_wall_spec(daily_at?)
    }

    /// The next time it is due after its last firing, or after `created`
    /// when it has never fired. Counted from the last firing, never from
    /// the times it missed: however long nothing fired (the server was down),
    /// it is due once, and one firing makes it due next in the future.
    pub fn next_after(&self, last: Option<DateTime<Utc>>, created: DateTime<Utc>, zone: Zone) -> Option<DateTime<Utc>> {
        let after = last.unwrap_or(created);
        match *self {
            Recurrence::EveryMinutes(m) => Some(after + chrono::Duration::minutes(m.max(1) as i64)),
            Recurrence::EveryHours(h) => Some(after + chrono::Duration::hours(h)),
            Recurrence::DailyAt { h, m } => match zone {
                Zone::Named(tz) => next_daily(after, last.is_some(), h, m, &tz),
                Zone::Server => next_daily(after, last.is_some(), h, m, &chrono::Local),
            },
            Recurrence::WeekdaysAt { h, m } => match zone {
                Zone::Named(tz) => next_matching_day(after, last.is_some(), h, m, &tz, |d| (weekday_bit(d.weekday()) & WEEKDAYS_MASK) != 0),
                Zone::Server => next_matching_day(after, last.is_some(), h, m, &chrono::Local, |d| (weekday_bit(d.weekday()) & WEEKDAYS_MASK) != 0),
            },
            Recurrence::WeeklyAt { mask, h, m } => match zone {
                Zone::Named(tz) => next_matching_day(after, last.is_some(), h, m, &tz, |d| (weekday_bit(d.weekday()) & mask) != 0),
                Zone::Server => next_matching_day(after, last.is_some(), h, m, &chrono::Local, |d| (weekday_bit(d.weekday()) & mask) != 0),
            },
            Recurrence::MonthlyAt { day, h, m } => match zone {
                Zone::Named(tz) => next_matching_day(after, last.is_some(), h, m, &tz, |d| d.day() == day.min(days_in_month(d.year(), d.month()))),
                Zone::Server => next_matching_day(after, last.is_some(), h, m, &chrono::Local, |d| d.day() == day.min(days_in_month(d.year(), d.month()))),
            },
            Recurrence::OnceAt { at, utc } => {
                if last.is_some() {
                    return None;
                }
                if utc {
                    Some(DateTime::from_naive_utc_and_offset(at, Utc))
                } else {
                    zone.earliest(&at).or_else(|| zone.earliest(&(at + chrono::Duration::hours(1))))
                }
            }
        }
    }
}

/// The next time a wall-clock schedule at `h:m` (in `tz`) on days matching
/// `matches_day` is due after `after`, which is its last firing when `fired`,
/// else its creation. Handles DST repeats and skips identically to `next_daily`.
fn next_matching_day<Tz: chrono::TimeZone>(after: DateTime<Utc>, fired: bool, h: u32, m: u32, tz: &Tz, matches_day: impl Fn(NaiveDate) -> bool) -> Option<DateTime<Utc>> {
    use chrono::LocalResult;
    let local = after.with_timezone(tz);
    let mut day = local.date_naive();
    for _ in 0..MAX_CALENDAR_SCAN_DAYS {
        if !matches_day(day) {
            day = day.succ_opt()?;
            continue;
        }
        let at = day.and_hms_opt(h, m, 0)?;
        let t = match tz.from_local_datetime(&at) {
            LocalResult::Single(t) => t,
            // In time order: chrono's `Local` gives the later first.
            LocalResult::Ambiguous(x, y) => {
                let (first, second) = if x <= y { (x, y) } else { (y, x) };
                if first > local {
                    first
                } else if !fired {
                    second
                } else {
                    day = day.succ_opt()?;
                    continue;
                }
            }
            LocalResult::None => {
                // An hour on is never in a gap; take its earlier time if repeated.
                match tz.from_local_datetime(&(at + chrono::Duration::hours(1))) {
                    LocalResult::Single(t) => t,
                    LocalResult::Ambiguous(x, y) => x.min(y),
                    LocalResult::None => return None,
                }
            }
        };
        if t > local {
            return Some(t.with_timezone(&Utc));
        }
        day = day.succ_opt()?;
    }
    None
}

/// The next time a daily schedule at `h:m` (wall-clock time in `tz`) is due
/// after `after`, which is its last firing when `fired`, else its creation.
fn next_daily<Tz: chrono::TimeZone>(after: DateTime<Utc>, fired: bool, h: u32, m: u32, tz: &Tz) -> Option<DateTime<Utc>> {
    next_matching_day(after, fired, h, m, tz, |_| true)
}

/// Extract a canonical wall-clock schedule string from structured fields
/// (`weekdays_at`, `weekly_at`, `monthly_at`, `once_at`) if present.
fn structured_wall(b: &serde_json::Value) -> Result<Option<String>, String> {
    if let Some(s) = b["weekdays_at"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        let (h, m) = parse_hm(s).ok_or_else(|| "weekdays_at must be HH:MM".to_string())?;
        return Ok(Some(format!("weekdays {h:02}:{m:02}")));
    }
    if let Some(w) = b.get("weekly_at").filter(|v| !v.is_null()) {
        let at = w["at"].as_str().ok_or_else(|| "weekly_at requires at as HH:MM".to_string())?;
        let (h, m) = parse_hm(at).ok_or_else(|| "weekly_at.at must be HH:MM".to_string())?;
        let mut mask = 0u8;
        if let Some(arr) = w["days"].as_array() {
            for d in arr {
                if let Some(s) = d.as_str() {
                    mask |= parse_weekday_token(s).ok_or_else(|| format!("unknown weekday {s:?}"))?;
                } else if let Some(n) = d.as_u64() {
                    mask |= parse_weekday_token(&n.to_string()).ok_or_else(|| format!("unknown weekday {n}"))?;
                }
            }
        } else if let Some(s) = w["days"].as_str() {
            mask = parse_weekday_mask(s).ok_or_else(|| "weekly_at.days must list weekdays".to_string())?;
        }
        if mask == 0 {
            return Err("weekly_at requires at least one weekday".into());
        }
        return Ok(Some(format!("weekly {} {h:02}:{m:02}", canonical_days(mask))));
    }
    if let Some(mo) = b.get("monthly_at").filter(|v| !v.is_null()) {
        let day = mo["day"].as_u64().filter(|d| (1..=31).contains(d)).ok_or_else(|| "monthly_at.day must be 1..=31".to_string())?;
        let at = mo["at"].as_str().ok_or_else(|| "monthly_at requires at as HH:MM".to_string())?;
        let (h, m) = parse_hm(at).ok_or_else(|| "monthly_at.at must be HH:MM".to_string())?;
        return Ok(Some(format!("monthly {day} {h:02}:{m:02}")));
    }
    if let Some(once) = b["once_at"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        parse_once_spec(once).ok_or_else(|| "once_at must be YYYY-MM-DDTHH:MM or RFC3339".to_string())?;
        return Ok(Some(format!("once {once}")));
    }
    Ok(None)
}

/// A schedule's rule as a request names it: `every_minutes` (at least `min`),
/// `daily_at` ("HH:MM", "weekdays HH:MM", "weekly mon,fri HH:MM",
/// "monthly 15 HH:MM", "once YYYY-MM-DDTHH:MM"), or structured `weekdays_at` /
/// `weekly_at` / `monthly_at` / `once_at` — exactly one of them.
pub fn rule_of(b: &serde_json::Value, min: u32) -> Result<(Option<u32>, Option<String>), String> {
    // A model that must fill every field sends 0 or "" for the one it does not mean.
    let every = b["every_minutes"].as_u64().filter(|m| *m > 0).map(|m| m as u32);
    let raw_daily = b["daily_at"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
    let structured = structured_wall(b)?;
    let daily = match (raw_daily, structured) {
        (Some(_), Some(_)) => return Err("give only one schedule cadence".into()),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    };
    match (every, &daily) {
        (Some(m), None) if m >= min => Ok((Some(m), None)),
        (Some(_), None) => Err(format!("schedules run at most every {min} minutes")),
        (None, Some(t)) if parse_wall_spec(t).is_some() => Ok((None, daily)),
        _ => Err("give either every_minutes or daily_at (HH:MM, weekdays HH:MM, weekly days HH:MM, monthly day HH:MM, or once YYYY-MM-DDTHH:MM)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A daily time in Los Angeles across both of its clock changes (owner's
    /// CI, 2026-11-01: schedules made in the repeated hour did not fire).
    #[test]
    fn a_daily_schedule_is_due_once_a_day_across_clock_changes() {
        use chrono::{TimeZone, Utc};
        let la = chrono_tz::America::Los_Angeles;
        let utc = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        // Clocks go back at 02:00 PDT on 2026-11-01; 01:00-02:00 comes twice.
        // Made in the first 01:14 (PDT), due at the first 01:16.
        assert_eq!(next_daily(utc("2026-11-01T08:14:00Z"), false, 1, 16, &la), Some(utc("2026-11-01T08:16:00Z")));
        // Made in the second 01:14 (PST), due at the second 01:16, not tomorrow.
        assert_eq!(next_daily(utc("2026-11-01T09:14:00Z"), false, 1, 16, &la), Some(utc("2026-11-01T09:16:00Z")));
        // Having fired at the first 01:16, not again at the second: tomorrow.
        assert_eq!(next_daily(utc("2026-11-01T08:16:00Z"), true, 1, 16, &la), Some(utc("2026-11-02T09:16:00Z")));
        // Fired the day before: the first 01:16.
        assert_eq!(next_daily(utc("2026-10-31T08:16:00Z"), true, 1, 16, &la), Some(utc("2026-11-01T08:16:00Z")));
        // Clocks go forward at 02:00 PST on 2027-03-14; 02:30 does not happen.
        // Due as they resume, at 03:30 PDT, and on the next day at 02:30 again.
        assert_eq!(next_daily(utc("2027-03-14T09:00:00Z"), false, 2, 30, &la), Some(utc("2027-03-14T10:30:00Z")));
        assert_eq!(next_daily(utc("2027-03-14T10:30:00Z"), true, 2, 30, &la), Some(utc("2027-03-15T09:30:00Z")));
        // Whatever order the time zone gives the two in (chrono's `Local`
        // gives the later first; on the owner's build box, the first 01:31
        // came out an hour late).
        let rev = Reversed(la);
        assert_eq!(next_daily(utc("2026-11-01T08:29:00Z"), false, 1, 31, &rev), Some(utc("2026-11-01T08:31:00Z")));
        assert_eq!(next_daily(utc("2026-11-01T09:29:00Z"), false, 1, 31, &rev), Some(utc("2026-11-01T09:31:00Z")));
        assert_eq!(next_daily(utc("2026-11-01T08:31:00Z"), true, 1, 31, &rev), Some(utc("2026-11-02T09:31:00Z")));
        // An ordinary day.
        assert_eq!(next_daily(la.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap().with_timezone(&Utc), false, 8, 0, &la), Some(utc("2026-06-02T15:00:00Z")));
    }

    /// A time zone that gives a repeated time's two instants latest first,
    /// as chrono's `Local` does.
    #[derive(Clone, Copy)]
    struct Reversed(chrono_tz::Tz);
    impl chrono::TimeZone for Reversed {
        type Offset = <chrono_tz::Tz as chrono::TimeZone>::Offset;
        fn from_offset(o: &Self::Offset) -> Self {
            Reversed(chrono_tz::Tz::from_offset(o))
        }
        fn offset_from_local_date(&self, d: &chrono::NaiveDate) -> chrono::LocalResult<Self::Offset> {
            self.0.offset_from_local_date(d)
        }
        fn offset_from_local_datetime(&self, d: &chrono::NaiveDateTime) -> chrono::LocalResult<Self::Offset> {
            match self.0.offset_from_local_datetime(d) {
                chrono::LocalResult::Ambiguous(a, b) => chrono::LocalResult::Ambiguous(b, a),
                r => r,
            }
        }
        fn offset_from_utc_date(&self, d: &chrono::NaiveDate) -> Self::Offset {
            self.0.offset_from_utc_date(d)
        }
        fn offset_from_utc_datetime(&self, d: &chrono::NaiveDateTime) -> Self::Offset {
            self.0.offset_from_utc_datetime(d)
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// A schedule's recorded timing: minutes win, an empty or broken time is none.
    #[test]
    fn a_schedules_timing_reads_as_its_recurrence() {
        assert_eq!(Recurrence::of_schedule(Some(0), Some("08:00")), Some(Recurrence::EveryMinutes(1)));
        assert_eq!(Recurrence::of_schedule(None, Some("08:05")), Some(Recurrence::DailyAt { h: 8, m: 5 }));
        assert_eq!(Recurrence::of_schedule(None, Some("")), None);
        assert_eq!(Recurrence::of_schedule(None, None), None);
    }

    /// Never fired: counted from its creation; fired: from the last firing.
    #[test]
    fn the_next_time_counts_from_the_last_firing_or_creation() {
        let made = utc("2026-11-14T10:00:00Z");
        let every = Recurrence::EveryHours(2);
        assert_eq!(every.next_after(None, made, Zone::Server), Some(utc("2026-11-14T12:00:00Z")));
        assert_eq!(every.next_after(Some(utc("2026-11-14T13:00:00Z")), made, Zone::Server), Some(utc("2026-11-14T15:00:00Z")));
    }

    /// The server down for hours: each recurrence is due once on its return,
    /// not once for every time it missed; the firing then makes it due next
    /// in the future.
    #[test]
    fn hours_with_the_server_down_fire_once_not_a_backlog() {
        let la = Zone::Named(chrono_tz::America::Los_Angeles);
        let made = utc("2026-11-01T00:00:00Z");
        let last = utc("2026-11-10T16:00:00Z");
        let back = utc("2026-11-13T22:00:00Z");
        for every in [Recurrence::EveryMinutes(5), Recurrence::EveryHours(1), Recurrence::DailyAt { h: 8, m: 0 }] {
            let due = every.next_after(Some(last), made, la).unwrap();
            assert!(due <= back, "{every:?} was not due on the server's return");
            let next = every.next_after(Some(back), made, la).unwrap();
            assert!(next > back, "{every:?} was due again at once: a backlog of {next}");
        }
    }

    /// A daily time in a named zone, through `next_after`: the skipped hour
    /// fires as clocks resume, the doubled hour once.
    #[test]
    fn a_daily_time_in_a_named_zone_fires_once_on_clock_change_days() {
        let la = Zone::Named(chrono_tz::America::Los_Angeles);
        let daily = Recurrence::DailyAt { h: 2, m: 30 };
        let made = utc("2027-03-14T09:00:00Z");
        assert_eq!(daily.next_after(None, made, la), Some(utc("2027-03-14T10:30:00Z")));
        let doubled = Recurrence::DailyAt { h: 1, m: 16 };
        assert_eq!(doubled.next_after(Some(utc("2026-11-01T08:16:00Z")), made, la), Some(utc("2026-11-02T09:16:00Z")));
    }
}

#[cfg(test)]
mod rule_tests {
    use super::rule_of;
    use serde_json::json;

    /// One of the two, `every_minutes` no shorter than the floor, a daily time
    /// a real one; a 0 or "" the model sends for the one it does not mean is
    /// not there.
    #[test]
    fn a_rule_is_every_minutes_or_a_daily_time() {
        assert_eq!(rule_of(&json!({ "every_minutes": 30, "daily_at": "" }), 15), Ok((Some(30), None)));
        assert_eq!(rule_of(&json!({ "every_minutes": 0, "daily_at": " 07:05 " }), 15), Ok((None, Some("07:05".into()))));
        assert!(rule_of(&json!({ "every_minutes": 5 }), 15).is_err());
        for bad in [json!({}), json!({ "daily_at": "24:00" }), json!({ "daily_at": "7:60" }), json!({ "every_minutes": 30, "daily_at": "07:00" })] {
            assert!(rule_of(&bad, 15).is_err(), "{bad}");
        }
    }

    #[test]
    fn calendar_grade_recurrences_parse_and_advance() {
        use super::{Recurrence, Zone};
        use chrono::{DateTime, Utc};
        let utc_zone = Zone::Named(chrono_tz::Tz::UTC);
        let ts = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);

        // Structured fields normalize to canonical strings.
        assert_eq!(rule_of(&json!({ "weekdays_at": "08:30" }), 15), Ok((None, Some("weekdays 08:30".into()))));
        assert_eq!(rule_of(&json!({ "weekly_at": { "days": ["fri", "mon"], "at": "16:00" } }), 15), Ok((None, Some("weekly mon,fri 16:00".into()))));
        assert_eq!(rule_of(&json!({ "monthly_at": { "day": 31, "at": "09:00" } }), 15), Ok((None, Some("monthly 31 09:00".into()))));
        assert_eq!(rule_of(&json!({ "once_at": "2026-12-01T09:00" }), 15), Ok((None, Some("once 2026-12-01T09:00".into()))));

        // Weekdays skips Saturday (2026-11-28) & Sunday (2026-11-29) -> Monday (2026-11-30).
        let wd = Recurrence::of_schedule(None, Some("weekdays 08:30")).unwrap();
        assert_eq!(wd.next_after(Some(ts("2026-11-27T08:30:00Z")), ts("2026-11-01T00:00:00Z"), utc_zone), Some(ts("2026-11-30T08:30:00Z")));

        // Weekly Friday at 16:00 from Tuesday 2026-11-24 -> Friday 2026-11-27.
        let wk = Recurrence::of_schedule(None, Some("weekly fri 16:00")).unwrap();
        assert_eq!(wk.next_after(None, ts("2026-11-24T10:00:00Z"), utc_zone), Some(ts("2026-11-27T16:00:00Z")));

        // Monthly on the 31st clamps to Feb 28 in 2027.
        let mo = Recurrence::of_schedule(None, Some("monthly 31 09:00")).unwrap();
        assert_eq!(mo.next_after(Some(ts("2027-01-31T09:00:00Z")), ts("2027-01-01T00:00:00Z"), utc_zone), Some(ts("2027-02-28T09:00:00Z")));

        // OnceAt fires once, then returns None after firing.
        let once = Recurrence::of_schedule(None, Some("once 2026-12-01T09:00")).unwrap();
        assert_eq!(once.next_after(None, ts("2026-11-24T00:00:00Z"), utc_zone), Some(ts("2026-12-01T09:00:00Z")));
        assert_eq!(once.next_after(Some(ts("2026-12-01T09:00:00Z")), ts("2026-11-24T00:00:00Z"), utc_zone), None);
    }
}
