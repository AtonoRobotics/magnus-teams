//! A daily schedule's time is its person's wall-clock time, in the zone
//! their preferences name, and the server's when they name none (owner,
//! 2026-11-14).

use crate::cadence::{Recurrence, Zone};
use crate::log::Event;
use crate::state::State;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

fn ev(s: &mut State, at: DateTime<Utc>, e: Value) {
    let seq = s.last_seq + 1;
    s.apply(&Event { seq, at, actor: "person:alice".into(), ev: serde_json::from_value(e).unwrap() });
}

fn zone(s: &mut State, at: DateTime<Utc>, tz: &str) {
    ev(s, at, json!({ "kind": "preferences_set", "person": "alice", "prefs": { "timezone": tz } }));
}

fn daily(s: &mut State, at: DateTime<Utc>, id: &str, time: &str) {
    ev(s, at, json!({ "kind": "schedule_set", "id": id, "person": "alice", "text": "Summarise my inbox.", "daily_at": time }));
}

/// alice is in Tokyo: her 08:00 is 23:00 UTC the day before, wherever the
/// server is; and a schedule she made before naming her zone follows it
/// once she does.
#[test]
fn a_daily_schedule_fires_at_its_persons_local_time() {
    let mut s = State::default();
    let made = utc("2026-11-14T12:00:00Z");
    daily(&mut s, made, "sc_before", "08:00");
    zone(&mut s, made, "Asia/Tokyo");
    daily(&mut s, made, "sc_after", "08:00");
    for id in ["sc_before", "sc_after"] {
        assert_eq!(s.arrangements.schedules()[id].next_due(), Some(utc("2026-11-14T23:00:00Z")), "{id} did not fire at 08:00 in Tokyo");
    }
}

/// The clock changes of the person's zone, not the server's: in Los
/// Angeles 02:30 does not happen on 2027-03-14, so it fires as the clocks
/// resume, and the next day at 02:30 again.
#[test]
fn a_daily_schedule_follows_the_clock_changes_of_its_persons_zone() {
    let mut s = State::default();
    let made = utc("2027-03-14T09:00:00Z");
    zone(&mut s, made, "America/Los_Angeles");
    daily(&mut s, made, "sc1", "02:30");
    assert_eq!(s.arrangements.schedules()["sc1"].next_due(), Some(utc("2027-03-14T10:30:00Z")));
    ev(&mut s, utc("2027-03-14T10:30:00Z"), json!({ "kind": "schedule_failed", "id": "sc1", "reason": "x" }));
    assert_eq!(s.arrangements.schedules()["sc1"].next_due(), Some(utc("2027-03-15T09:30:00Z")));
}

/// A person who names no zone, or one that is not a zone, keeps the
/// server's clock, exactly as before.
#[test]
fn a_person_with_no_zone_keeps_the_servers_clock() {
    let made = utc("2026-11-14T12:00:00Z");
    let servers = Recurrence::DailyAt { h: 8, m: 0 }.next_after(None, made, Zone::Server);
    let mut s = State::default();
    daily(&mut s, made, "sc_none", "08:00");
    assert_eq!(s.arrangements.schedules()["sc_none"].zone, Zone::Server);
    assert_eq!(s.arrangements.schedules()["sc_none"].next_due(), servers);
    zone(&mut s, made, "Mars/Olympus");
    daily(&mut s, made, "sc_bad", "08:00");
    assert_eq!(s.arrangements.schedules()["sc_bad"].zone, Zone::Server, "a zone that is not one was used");
    assert_eq!(s.arrangements.schedules()["sc_bad"].next_due(), servers);
}

/// A person who names no zone has one zone for everything: their quiet
/// hours end at the moment their daily schedule at the same time fires,
/// and both are the server's clock, not UTC.
#[test]
fn a_person_with_no_zone_has_the_same_zone_for_schedules_and_quiet_hours() {
    use chrono::TimeZone;
    let mut s = State::default();
    // 03:00 tomorrow on the server's clock, inside the default quiet hours
    // (22:00 to 07:00).
    let day = (chrono::Local::now() + chrono::Duration::days(1)).date_naive();
    let now = chrono::Local.from_local_datetime(&day.and_hms_opt(3, 0, 0).unwrap()).earliest().unwrap().with_timezone(&Utc);
    daily(&mut s, now, "sc", "07:00");
    let prefs = s.identity.preferences_of("alice").cloned().unwrap_or(Value::Null);
    let st = crate::proactive::settings(&crate::config::Proactive::default(), &prefs);
    let sc = &s.arrangements.schedules()["sc"];
    assert_eq!(st.tz, sc.zone, "quiet hours and schedules read different zones");
    assert_eq!(crate::proactive::quiet_ends_at(now, st.tz, st.quiet), sc.next_due(), "quiet hours did not end when the 07:00 schedule fires");
    assert_eq!(crate::proactive::day_key(now, st.tz), day.to_string(), "the person's day is not the server's");
}
