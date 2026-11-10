//! The Outlook calendar server's own pieces: its tools against its
//! manifest, and an event as Graph takes and gives it.

use super::*;

#[test]
fn every_tool_is_classed_in_the_manifest() {
    let m: toml::Table = include_str!("../../catalog/microsoft-calendar.toml").parse().unwrap();
    let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
    let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
    classed.sort();
    listed.sort();
    assert_eq!(classed, listed);
    // Each that changes an event tells the people invited, so each asks.
    for t in ["create_event", "update_event", "respond_to_event"] {
        assert_eq!(m["tools"][t].as_str(), Some("irreversible"), "{t}");
    }
    for t in tools() {
        assert_eq!(t.input["type"], "object", "{}", t.name);
        assert_eq!(t.output["type"], "object", "{}", t.name);
    }
}

/// An event is written with only the fields a call names, its times local
/// in a named zone (UTC for one ending in Z), a whole day from midnight,
/// and its people by address.
#[test]
fn an_event_is_written_as_graph_takes_it() {
    let b = event_body(
        &json!({ "subject": "Plan", "start_time": "2026-11-04T10:00:00", "end_time": "2026-11-04T10:30:00", "time_zone": "Europe/London", "attendees": ["bo@example.com"] }),
    )
    .unwrap();
    assert_eq!(
        b,
        json!({ "subject": "Plan", "start": { "dateTime": "2026-11-04T10:00:00", "timeZone": "Europe/London" }, "end": { "dateTime": "2026-11-04T10:30:00", "timeZone": "Europe/London" }, "attendees": [{ "emailAddress": { "address": "bo@example.com" }, "type": "required" }] })
    );
    assert_eq!(
        event_body(&json!({ "start_time": "2026-11-04T18:00:00Z", "time_zone": "Europe/London" })).unwrap(),
        json!({ "start": { "dateTime": "2026-11-04T18:00:00", "timeZone": "UTC" } })
    );
    assert_eq!(
        event_body(&json!({ "start_time": "2026-11-04T09:00:00", "end_time": "2026-11-05", "all_day": true })).unwrap(),
        json!({ "isAllDay": true, "start": { "dateTime": "2026-11-04T00:00:00", "timeZone": "UTC" }, "end": { "dateTime": "2026-11-05T00:00:00", "timeZone": "UTC" } })
    );
    assert!(event_body(&json!({ "start_time": "2026-11-04T10:00:00-08:00" })).unwrap_err().contains("names an offset"));
    assert_eq!(event_body(&json!({ "location": "Room 2" })).unwrap(), json!({ "location": { "displayName": "Room 2" } }));
    assert_eq!(cal(&json!({})), "/v1.0/me/");
    assert_eq!(cal(&json!({ "calendar_id": "AAMk/x=" })), "/v1.0/me/calendars/AAMk%2Fx%3D/");
}

#[test]
fn an_answer_is_one_of_graph_s_three() {
    assert_eq!(answer("accepted"), Ok("accept"));
    assert_eq!(answer("declined"), Ok("decline"));
    assert_eq!(answer("tentative"), Ok("tentativelyAccept"));
    assert!(answer("maybe").is_err());
}

#[test]
fn an_event_is_read_as_a_person_reads_it() {
    let e = json!({ "id": "e1", "subject": "Plan", "start": { "dateTime": "2026-11-04T18:00:00.0000000", "timeZone": "UTC" }, "end": { "dateTime": "2026-11-04T18:30:00.0000000", "timeZone": "UTC" },
        "organizer": { "emailAddress": { "address": "bo@example.com" } }, "responseStatus": { "response": "notResponded" },
        "attendees": [{ "emailAddress": { "address": "al@example.com" }, "type": "required", "status": { "response": "accepted" } }] });
    let v = event(&e);
    assert_eq!((v["event_id"].as_str(), v["start"].as_str(), v["time_zone"].as_str()), (Some("e1"), Some("2026-11-04T18:00:00.0000000"), Some("UTC")));
    assert_eq!((v["organizer"].as_str(), v["my_response"].as_str()), (Some("bo@example.com"), Some("notResponded")));
    assert_eq!((v["attendees"][0]["email"].as_str(), v["attendees"][0]["response"].as_str()), (Some("al@example.com"), Some("accepted")));
}
