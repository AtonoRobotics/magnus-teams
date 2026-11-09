//! The Calendar server's own pieces: its tools against its manifest, an
//! event as Calendar takes and gives it, and free times.

use super::*;

#[test]
fn every_tool_is_classed_in_the_manifest() {
    let m: toml::Table = include_str!("../../catalog/google-calendar.toml").parse().unwrap();
    let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
    let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
    classed.sort();
    listed.sort();
    assert_eq!(classed, listed);
    // Each that changes an event tells the people invited, so each asks.
    for t in ["create_event", "update_event", "delete_event", "respond_to_event"] {
        assert_eq!(m["tools"][t].as_str(), Some("irreversible"), "{t}");
    }
}

#[test]
fn every_tool_names_its_schemas() {
    let ts = tools();
    assert_eq!(ts.len(), 9);
    for t in &ts {
        assert_eq!(t.input["type"], "object", "{}", t.name);
        assert_eq!(t.output["type"], "object", "{}", t.name);
    }
}

/// An event is written with only the fields a call names, a whole day as
/// a date, and its people by email; ids go in paths encoded.
#[test]
fn an_event_is_written_as_calendar_takes_it() {
    let b = event_body(
        &json!({ "summary": "Plan", "start_time": "2026-11-04T10:00:00-08:00", "end_time": "2026-11-04T10:30:00-08:00", "time_zone": "America/Los_Angeles", "attendees": ["bo@example.com"] }),
    );
    assert_eq!(
        b,
        json!({ "summary": "Plan", "start": { "dateTime": "2026-11-04T10:00:00-08:00", "timeZone": "America/Los_Angeles" }, "end": { "dateTime": "2026-11-04T10:30:00-08:00", "timeZone": "America/Los_Angeles" }, "attendees": [{ "email": "bo@example.com" }] })
    );
    let day = event_body(&json!({ "start_time": "2026-11-04T00:00:00Z", "end_time": "2026-11-05", "all_day": true }));
    assert_eq!(day, json!({ "start": { "date": "2026-11-04" }, "end": { "date": "2026-11-05" } }));
    assert_eq!(event_body(&json!({ "location": "Room 2" })), json!({ "location": "Room 2" }));
    assert_eq!(cal(&json!({})), "primary");
    assert_eq!(cal(&json!({ "calendar_id": "team@group.calendar.google.com" })), "team%40group.calendar.google.com");
    assert_eq!(event_id(&json!({ "event_id": "../x" })).unwrap(), "..%2Fx");
    assert!(event_id(&json!({})).is_err());
    assert_eq!(updates(&json!({})), "all");
    assert_eq!(updates(&json!({ "notify": false })), "none");
}

/// An answer changes only the person's own place in the list, and every
/// other person stays invited.
#[test]
fn an_answer_keeps_everyone_else_invited() {
    let e = json!({ "attendees": [{ "email": "bo@example.com", "organizer": true, "responseStatus": "accepted" }, { "email": "al@example.com", "self": true, "responseStatus": "needsAction" }, { "email": "cy@example.com", "responseStatus": "declined" }] });
    let b = answered(&e, "tentative", Some("Maybe late")).unwrap();
    assert_eq!(b["attendees"].as_array().unwrap().len(), 3);
    assert_eq!(b["attendees"][0], e["attendees"][0]);
    assert_eq!(b["attendees"][2], e["attendees"][2]);
    assert_eq!((b["attendees"][1]["responseStatus"].as_str(), b["attendees"][1]["comment"].as_str()), (Some("tentative"), Some("Maybe late")));
    assert!(answered(&json!({ "attendees": [{ "email": "bo@example.com" }] }), "accepted", None).is_err());
}

#[test]
fn an_event_is_read_as_a_person_reads_it() {
    let e = json!({ "id": "e1", "summary": "Plan", "start": { "dateTime": "2026-11-04T10:00:00-08:00", "timeZone": "America/Los_Angeles" }, "end": { "dateTime": "2026-11-04T10:30:00-08:00" }, "organizer": { "email": "bo@example.com" }, "attendees": [{ "email": "al@example.com", "self": true, "responseStatus": "accepted" }] });
    let v = event(&e);
    assert_eq!((v["event_id"].as_str(), v["start"].as_str(), v["all_day"].as_bool()), (Some("e1"), Some("2026-11-04T10:00:00-08:00"), Some(false)));
    assert_eq!((v["attendees"][0]["me"].as_bool(), v["attendees"][0]["response"].as_str()), (Some(true), Some("accepted")));
    assert_eq!(event(&json!({ "start": { "date": "2026-11-04" } }))["all_day"], true);
}

#[test]
fn times_are_read_and_written_as_rfc_3339() {
    assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(epoch("2026-11-04T10:00:00-08:00"), epoch("2026-11-04T18:00:00Z"));
    assert_eq!(epoch("2026-11-04T18:00:00.250Z"), epoch("2026-11-04T18:00:00Z"));
    assert_eq!(epoch("2024-02-29T12:00:00+05:30").map(rfc3339).as_deref(), Some("2024-02-29T06:30:00Z"));
    assert_eq!(rfc3339(epoch("2026-12-31T23:59:59Z").unwrap()), "2026-12-31T23:59:59Z");
    assert_eq!(epoch("tomorrow"), None);
    assert_eq!(epoch("2026-11-04T10:00:00"), None);
}

/// Free times start on the quarter hour, skip every busy time, and stop
/// at the window's end or the number asked for.
#[test]
fn free_times_fall_between_busy_ones() {
    let at = |t: &str| epoch(&format!("2026-11-04T{t}:00Z")).unwrap();
    let busy = vec![(at("10:30"), at("11:10")), (at("09:00"), at("10:00"))];
    let slots: Vec<(String, String)> = free(at("09:05"), at("12:00"), 30, busy.clone(), 10).into_iter().map(|(s, e)| (rfc3339(s), rfc3339(e))).collect();
    let want = [("10:00", "10:30"), ("11:15", "11:45")].map(|(s, e)| (format!("2026-11-04T{s}:00Z"), format!("2026-11-04T{e}:00Z")));
    assert_eq!(slots, want);
    assert_eq!(free(at("09:00"), at("12:00"), 30, busy.clone(), 1).len(), 1);
    assert!(free(at("09:00"), at("10:00"), 30, busy, 3).is_empty());
}

/// Free and busy times are read from every calendar, and a calendar or a
/// group Google could not check is named as not checked, groups apart
/// from calendars (review of #126).
#[test]
fn a_group_google_could_not_list_is_named_as_not_checked() {
    let r = json!({
        "calendars": { "primary": { "busy": [{ "start": "2026-11-05T13:00:00Z", "end": "2026-11-05T14:00:00Z" }] }, "gone@example.com": { "errors": [{ "domain": "calendar", "reason": "notFound" }] } },
        "groups": { "team@example.com": { "errors": [{ "domain": "calendar", "reason": "groupTooBig" }] }, "ok@example.com": { "calendars": ["primary"] } },
    });
    let (busy, mut unknown) = busy_times(&r);
    unknown.sort();
    assert_eq!(busy, [(epoch("2026-11-05T13:00:00Z").unwrap(), epoch("2026-11-05T14:00:00Z").unwrap())]);
    assert_eq!(unknown, ["gone@example.com", "team@example.com"]);
}
