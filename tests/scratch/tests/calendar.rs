//! Google Calendar, brokered (connectors spec, Brokered API connectors;
//! Google's hosted Calendar server refuses Magnus with 403). The catalog's
//! Calendar manifest names the image built from this checkout
//! (`brokered/calendar`), which runs in each person's own microVM and
//! reaches only `www.googleapis.com`, through the appliance's egress proxy.
//! A local HTTPS host stands in for Calendar's API, with a certificate for
//! its name from the test's own CA, and FakeOAuthApp for Google's sign-in
//! (the real ones need a Google account: `calendar_contract.rs` uses the
//! owner's test account).

use magnus_invariants::*;
use magnus_scratch::stand_in::*;
use serde_json::{json, Value};

const HOST: &str = "www.googleapis.com";
const BASE: &str = "/calendar/v3";
const WRITES: [&str; 4] = ["create_event", "update_event", "delete_event", "respond_to_event"];

/// What Calendar answers, for the requests the tests make: one event on
/// the person's calendar, to which Bob invited her; an event added comes
/// back as sent; Bob is busy from ten to eleven, and she from one to two,
/// and Google could not list the team's members; her calendars come in two
/// pages.
fn calendar(r: &Request, _: &[Request]) -> (u16, String, String) {
    let invite = json!({ "id": "e1", "summary": "Plan review", "start": { "dateTime": "2026-11-05T09:00:00Z" }, "end": { "dateTime": "2026-11-05T09:30:00Z" }, "organizer": { "email": "bob@example.com" },
        "attendees": [{ "email": "bob@example.com", "organizer": true, "responseStatus": "accepted" }, { "email": "alice@example.com", "self": true, "responseStatus": "needsAction" }] });
    let body: Value = serde_json::from_str(&r.body).unwrap_or_default();
    match (r.method.as_str(), r.path.strip_prefix(BASE).unwrap_or("").split('?').next().unwrap_or("")) {
        ("GET", "/calendars/primary/events") => ok(json!({ "items": [invite] })),
        ("POST", "/calendars/primary/events") => {
            let mut e = body;
            e["id"] = json!("e2");
            ok(e)
        }
        ("POST", "/freeBusy") => ok(json!({ "calendars": {
            "primary": { "busy": [{ "start": "2026-11-05T13:00:00Z", "end": "2026-11-05T14:00:00Z" }] },
            "bob@example.com": { "busy": [{ "start": "2026-11-05T10:00:00Z", "end": "2026-11-05T11:00:00Z" }] } },
            "groups": { "team@example.com": { "errors": [{ "domain": "calendar", "reason": "groupTooBig" }] } } })),
        ("GET", "/users/me/calendarList") if r.path.contains("pageToken=p2") => {
            ok(json!({ "items": [{ "id": "team@group.calendar.google.com", "summary": "Team", "accessRole": "reader" }] }))
        }
        ("GET", "/users/me/calendarList") => {
            ok(json!({ "items": [{ "id": "alice@example.com", "summary": "Alice", "primary": true, "accessRole": "owner" }], "nextPageToken": "p2" }))
        }
        _ => not_found(),
    }
}

/// Adding an event invites people, which cannot be called back, so it
/// always asks first: here Magnus has read nothing yet (nothing else makes
/// it ask), and nothing reaches Calendar until Alice approves it.
#[tokio::test(flavor = "multi_thread")]
async fn calendar_adds_an_event_only_once_the_person_approves_it_reaching_only_google() {
    let api = StandIn::start(HOST, calendar).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("invite Bob to a planning meeting on Thursday at ten"),
            1 => tool(
                "calendar__create_event",
                json!({ "summary": "Planning", "start_time": "2026-11-05T10:00:00Z", "end_time": "2026-11-05T10:30:00Z", "attendees": ["bob@example.com"] }),
            ),
            _ => complete("Bob is invited"),
        }
    }))
    .await;
    let (app, provider, _image) = signed_in(&m, &api, "google-calendar", "magnus-calendar", "calendar").await;
    let alice = app.api("alice");

    let task = alice.submit("Invite Bob to a planning meeting on Thursday at ten.").await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert_eq!(tool, "create_event");
    assert!(api.requests().is_empty(), "Calendar was called before Alice approved: {:?}", api.calls(BASE));
    approve(&alice, &ask).await;

    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    assert_eq!(api.calls(BASE), [("POST".to_string(), "/calendars/primary/events".to_string())]);
    let sent = &api.requests()[0];
    assert!(sent.path.ends_with("?sendUpdates=all"), "{sent:?}");
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(
        body,
        json!({ "summary": "Planning", "start": { "dateTime": "2026-11-05T10:00:00Z" }, "end": { "dateTime": "2026-11-05T10:30:00Z" }, "attendees": [{ "email": "bob@example.com" }] })
    );
    assert!(told(&m, "Invite Bob to a planning meeting").contains("\\\"event_id\\\":\\\"e2\\\""), "the event's answer did not reach Magnus");
    only_the_api(&app, &api, &provider, BASE);
}

/// Every calendar comes back, page after page (review of #126).
#[tokio::test(flavor = "multi_thread")]
async fn calendar_lists_every_calendar_across_pages() {
    let api = StandIn::start(HOST, calendar).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("which calendars do I have"),
            1 => tool("calendar__list_calendars", json!({})),
            _ => complete("yours and the team's"),
        }
    }))
    .await;
    let (app, provider, _image) = signed_in(&m, &api, "google-calendar", "magnus-calendar", "calendar").await;
    let alice = app.api("alice");
    let task = alice.submit("Which calendars do I have?").await;
    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    let get = ("GET".to_string(), "/users/me/calendarList".to_string());
    assert_eq!(api.calls(BASE), [get.clone(), get]);
    assert!(api.requests()[1].path.contains("pageToken=p2"), "{:?}", api.requests()[1]);
    let said = told(&m, "Which calendars do I have?");
    assert!(
        said.contains("alice@example.com") && said.contains("team@group.calendar.google.com") && said.contains("\\\"more\\\":false"),
        "not every calendar reached Magnus: {said}"
    );
    only_the_api(&app, &api, &provider, BASE);
}

/// A time that works is found from when each person is busy, and finding
/// it asks nothing: it changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn calendar_finds_a_time_when_everyone_is_free() {
    let api = StandIn::start(HOST, calendar).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("find an hour Bob and I are both free on Thursday"),
            1 => tool("calendar__suggest_time", json!({ "start_time": "2026-11-05T10:00:00Z", "end_time": "2026-11-05T15:00:00Z", "duration_minutes": 60, "attendees": ["bob@example.com", "team@example.com"], "max_suggestions": 3 })),
            _ => complete("Thursday at eleven, noon or two"),
        }
    }))
    .await;
    let (app, provider, _image) = signed_in(&m, &api, "google-calendar", "magnus-calendar", "calendar").await;
    let alice = app.api("alice");

    let task = alice.submit("Find an hour Bob and I are both free on Thursday.").await;
    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    assert_eq!(api.calls(BASE), [("POST".to_string(), "/freeBusy".to_string())]);
    let asked_for: Value = serde_json::from_str(&api.requests()[0].body).unwrap();
    assert_eq!(asked_for["items"], json!([{ "id": "primary" }, { "id": "bob@example.com" }, { "id": "team@example.com" }]));
    let said = told(&m, "Find an hour Bob and I are both free");
    // Bob is busy from ten, she from one: free at eleven, noon and two.
    let slot = |s: &str, e: &str| format!("{{\\\"end\\\":\\\"2026-11-05T{e}:00:00Z\\\",\\\"start\\\":\\\"2026-11-05T{s}:00:00Z\\\"}}");
    let want = format!("\\\"suggestions\\\":[{},{},{}]", slot("11", "12"), slot("12", "13"), slot("14", "15"));
    assert!(said.contains(&want), "the free times did not reach Magnus as {want}: {said}");
    // The team's members could not be listed, so their times are not known
    // (review of #126).
    assert!(said.contains("\\\"not_checked\\\":[\\\"team@example.com\\\"]"), "the team was not said to be unchecked: {said}");
    only_the_api(&app, &api, &provider, BASE);
}

/// Google's refusals reach the boundary as what they mean: a sign-in
/// without Calendar's access asks the person for it with the app's card,
/// and an event added with no clear answer is an unknown outcome, never a
/// failure that could be tried again as if no one had been invited.
#[tokio::test(flavor = "multi_thread")]
async fn calendar_s_refusals_reach_the_boundary_as_what_they_mean() {
    let api = StandIn::start(HOST, calendar).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("invite Bob to the plan review"),
            1 => tool("calendar__list_events", json!({ "start_time": "2026-11-05T00:00:00Z" })),
            2 => tool(
                "calendar__create_event",
                json!({ "summary": "Plan review", "start_time": "2026-11-05T10:00:00Z", "end_time": "2026-11-05T10:30:00Z", "attendees": ["bob@example.com"] }),
            ),
            _ => complete("done"),
        }
    }))
    .await;
    let (app, provider, _image) = signed_in(&m, &api, "google-calendar", "magnus-calendar", "calendar").await;
    let alice = app.api("alice");
    let scope = "https://www.googleapis.com/auth/calendar";
    api.then(
        403,
        &format!("WWW-Authenticate: Bearer realm=\"https://accounts.google.com/\", error=\"insufficient_scope\", scope=\"{scope}\"\r\n"),
        json!({ "error": { "code": 403, "message": "Request had insufficient authentication scopes.", "errors": [{ "reason": "insufficientPermissions" }] } }),
    );
    let task = alice.submit("Invite Bob to the plan review.").await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert!(told(&m, "Invite Bob to the plan review.").contains("needs more access from the person for list_events"), "the refusal was not a request for access");
    assert_eq!(tool, "create_event");
    // The add: Calendar breaks off with 503, so the invitation may have gone.
    api.then(503, "", json!({ "error": { "code": 503, "message": "The service is currently unavailable." } }));
    approve(&alice, &ask).await;
    eventually("the add's unknown outcome", std::time::Duration::from_secs(60), || async {
        app.logs().contains("create_event: outcome unknown: Google Calendar answered HTTP 503 and whether it did this is unknown.").then_some(())
    })
    .await;
    only_the_api(&app, &api, &provider, BASE);
}
