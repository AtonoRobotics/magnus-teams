//! The person's apps over MCP, added from the app: the owner adds one by its
//! address, signs in under MCP's authorization spec (the code comes back to
//! a loopback address on their computer), classifies its tools (the app's
//! hints only suggest), and the worker is offered only what was classified.
//! Tokens refresh as they expire; the kill switch revokes them at the app.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

fn tool_of<'a>(list: &'a Value, app: &str, tool: &str) -> &'a Value {
    let c = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == app).expect("the app is listed");
    c["tools"].as_array().unwrap().iter().find(|t| t["name"] == tool).unwrap_or(&Value::Null)
}

/// The organisation provides an app; each person signs in to it with their
/// own account, and only their own runs use that sign-in (connectors spec;
/// owner, 2026-11-01: "connectors are per person login. The connector
/// itself is provided by the org.").
async fn sign_in(api: &Api, app: &str, port: u16) {
    let (st, v) = api.post(&format!("/v1/connectors/{app}/sign-in"), json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(state, v["state"].as_str().unwrap());
    assert_eq!(api.post(&format!("/v1/connectors/{app}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
}

/// A provider may not know a client it registered a moment ago: GitLab.com
/// answered the sign-in address for one with 401 invalid_client, "The
/// requested OAuth 2.0 Client does not exist" (master run 42105845733,
/// #170). Magnus hands the person the address once the provider knows the
/// client, so her sign-in goes through.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_the_provider_does_not_know_yet_is_waited_for_before_the_person_signs_in() {
    let cal = FakeOAuthApp::start(3600).await;
    cal.state.lock().unwrap().new_clients_unknown_at_first = true;
    let m = start_checked(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sign_in(&alice, "calendar", callback.local_addr().unwrap().port()).await;
    let (_, list) = alice.get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], true, "{list}");
}

/// The person waits for the sign-in page ten seconds at most, whatever the
/// provider does: here it says it does not know the client for nine
/// seconds, then answers nothing. A request started near the end used to
/// get ten seconds of its own (review of #209), so against nine seconds of
/// answers that broken shape takes about nineteen, against ten for the one
/// global deadline; past ten the old loop's own deadline would fire first
/// and the two would be indistinguishable (#303).
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_waits_for_a_new_client_ten_seconds_at_most() {
    let cal = FakeOAuthApp::start(3600).await;
    cal.state.lock().unwrap().new_clients_unknown_then_silent = Some(std::time::Duration::from_secs(9));
    let m = start_checked(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let started = std::time::Instant::now();
    let (st, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{}/callback", callback.local_addr().unwrap().port()) })).await;
    let waited = started.elapsed();
    assert!(st.is_success() && v["authorize_url"].is_string(), "{v}");
    // Twelve seconds broke twice under overlapping full suites (runs
    // 42261795000 and 42262894435: 12.6 s, 14.0 s), and eighteen would let
    // the #209 shape pass against a seven-second fixture (review of #283:
    // seven plus its own ten is seventeen). At nine the broken shape takes
    // about nineteen, so sixteen keeps two seconds over the slowest loaded
    // pass and three under the regression.
    assert!(waited < std::time::Duration::from_secs(16), "the person waited {waited:?} for the sign-in page");
}

/// Magnus registers at an app's authorization server once: every later
/// sign-in, by anyone on the appliance and from whatever port on their
/// computer, uses that client. GitLab.com lets one address register about
/// ten clients an hour (#208).
#[tokio::test(flavor = "multi_thread")]
async fn sign_ins_by_anyone_from_any_port_register_magnus_once() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    for who in ["alice", "bob", "alice"] {
        let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        sign_in(&app.api(who), "calendar", callback.local_addr().unwrap().port()).await;
    }
    let (_, list) = app.api("bob").get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], true, "{list}");
    assert_eq!(cal.state.lock().unwrap().registrations, 1, "Magnus registered again for a later sign-in");
}

/// A provider can drop a client Magnus keeps: the next sign-in finds its
/// sign-in refusing the client, registers again, and goes through.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_the_provider_dropped_is_registered_again() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    sign_in(&app.api("alice"), "calendar", port).await;
    cal.forget_clients();
    sign_in(&app.api("bob"), "calendar", port).await;
    let (_, list) = app.api("bob").get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], true, "{list}");
    assert_eq!(cal.state.lock().unwrap().registrations, 2);
}

/// A provider's sign-in that answers 429 says nothing about the client:
/// Magnus keeps it and spends no registration (review of #215).
#[tokio::test(flavor = "multi_thread")]
async fn a_busy_provider_does_not_cost_a_registration() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    sign_in(&app.api("alice"), "calendar", port).await;
    cal.state.lock().unwrap().sign_in_throttled = true;
    let (st, v) = app.api("bob").post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    assert!(st.is_success() && v["authorize_url"].is_string(), "{v}");
    assert_eq!(cal.state.lock().unwrap().registrations, 1, "a 429 cost a registration");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_adds_an_app_each_person_signs_in_with_their_own_account_and_the_kill_switch_revokes_every_sign_in() {
    // Tokens last 30 s, inside the minute Magnus refreshes ahead: every use refreshes.
    let cal = FakeOAuthApp::start(30).await;
    let m = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("add the standup", "the standup is on the calendar"),
            1 => tool("calendar__create_event", json!({ "title": "Standup" })),
            2 => tool("calendar__list_events", json!({})),
            _ => complete("added"),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let add = json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" });

    // Only the owner adds apps; the app itself says it needs a sign-in.
    assert_eq!(carol.post("/v1/connectors", add.clone()).await.0, 403);
    let (st, v) = alice.post("/v1/connectors", add.clone()).await;
    assert_eq!((st.as_u16(), v["sign_in"].clone()), (201, json!(true)), "{v}");
    assert_eq!(alice.post("/v1/connectors", add).await.0, 409);
    let (_, list) = alice.get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], false, "{list}");

    // The code goes only to the person's own computer.
    let (st, _) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": "https://elsewhere.example/cb" })).await;
    assert!(!st.is_success(), "a sign-in redirected off the person's computer");

    // The desktop app listens on a loopback address and opens the browser there.
    // Held for the whole test, as the app holds its own, so no other test gets the port.
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    let (st, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    // Someone else cannot finish another person's sign-in and make it theirs.
    assert!(!carol.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0.is_success(), "carol took alice's sign-in");
    let (st, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);
    assert!(!alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0.is_success(), "a sign-in completed twice");

    // Its tools, each with what the app's hints suggest, none offered yet.
    let (_, list) = alice.get("/v1/connectors").await;
    for (t, suggested) in [("list_events", "read"), ("create_event", "reversible_write"), ("delete_all", "irreversible")] {
        let tool = tool_of(&list, "calendar", t);
        assert_eq!((tool["suggested"].as_str(), tool["risk"].clone()), (Some(suggested), Value::Null), "{list}");
    }
    assert_eq!(carol.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "read" })).await.0, 403);
    assert_eq!(alice.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "read" })).await.0, 204);
    assert_eq!(alice.put("/v1/connectors/calendar/tools/create_event", json!({ "risk": "reversible_write" })).await.0, 204);
    assert_eq!(alice.put("/v1/connectors/calendar/tools/delete_all", json!({ "risk": "anything" })).await.0, 400);
    assert!(alice.put_policy("org", &org_policy(&permit_execute("calendar__create_event"))).await.is_success());

    // Carol sees the organisation's app, not signed in for her: alice's
    // sign-in is alice's. Her task is not offered it, and a call is refused.
    let (_, list) = carol.get("/v1/connectors").await;
    assert_eq!((list["connectors"][0]["name"].as_str(), list["connectors"][0]["signed_in"].as_bool()), (Some("calendar"), Some(false)), "{list}");
    assert!(list["connectors"][0]["tools"].is_null() && list["connectors"][0]["url"].is_null(), "a person sees the owner's view: {list}");
    let before = m.requests().len();
    let t = carol.submit("Put the standup on my calendar.").await;
    carol.wait_settled(&t).await;
    assert_eq!(cal.calls("create_event"), 0, "carol's task acted through alice's sign-in");
    let offered: Vec<String> = m.requests()[before..].iter().flat_map(|c| c.offered_tools()).collect();
    assert!(!offered.iter().any(|t| t.starts_with("calendar__")), "carol was offered an app she has not signed in to: {offered:?}");

    // Carol signs in with her own account; her task uses the classified tools.
    sign_in(&carol, "calendar", port).await;
    let before = m.requests().len();
    let t = carol.submit("Put the standup on my calendar.").await;
    let done = carol.wait_settled(&t).await;
    assert_eq!(done["waiting_on"], "review", "{done}");
    assert_eq!((cal.calls("create_event"), cal.calls("list_events")), (1, 1));
    let offered: Vec<String> = m.requests()[before..].iter().flat_map(|c| c.offered_tools()).collect();
    assert!(offered.contains(&"calendar__create_event".to_string()) && !offered.iter().any(|t| t == "calendar__delete_all"), "{offered:?}");
    assert!(cal.state.lock().unwrap().refreshes >= 2, "expired tokens were not refreshed (or a rotated refresh token was reused)");

    // Alice ends her own sign-in; carol's is untouched and still hers.
    let (st, ended) = alice.send(reqwest::Method::DELETE, "/v1/connectors/calendar/connection", None).await;
    assert_eq!((st.as_u16(), ended["at_provider"].as_bool()), (200, Some(true)), "{ended}");
    assert_eq!(alice.get("/v1/connectors").await.1["connectors"][0]["signed_in"], false);
    assert_eq!(carol.get("/v1/connectors").await.1["connectors"][0]["signed_in"], true);

    // The kill switch revokes every person's sign-in at the app.
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    let ks = eventually("carol's sign-in is revoked", PATIENCE, || async {
        let (_, ks) = alice.get("/v1/kill-switch").await;
        (ks["revoked"]["connector_calendar__carol"]["at_provider"] == true).then_some(ks)
    })
    .await;
    assert!(cal.state.lock().unwrap().revoked.iter().any(|t| t.starts_with("rt-")), "{ks}");
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
    let (_, list) = carol.get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], false, "a revoked sign-in still counts: {list}");

    // Removed, it is gone; the worker is no longer offered its tools.
    let (st, _) = alice.send(reqwest::Method::DELETE, "/v1/connectors/calendar", None).await;
    assert_eq!(st, 204);
    let (_, list) = alice.get("/v1/connectors").await;
    assert!(!list["connectors"].as_array().unwrap().iter().any(|c| c["name"] == "calendar"), "{list}");
}

/// A tool whose definition the app changes after the worker first saw it is
/// disabled until the owner accepts the change (B10); the owner decides it
/// from the inbox item, shown again on the connector in Settings.
/// Someone else's attempt at a person's sign-in is refused and leaves it
/// theirs to finish: it does not end it for them.
#[tokio::test(flavor = "multi_thread")]
async fn another_persons_attempt_leaves_a_sign_in_waiting_for_its_owner() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(script(|_| async move { complete("done") })).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    assert_eq!(alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await.0, 201);
    // Held for the whole test, as the app holds its own, so no other test gets the port.
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    let (_, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert!(!carol.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0.is_success(), "carol took alice's sign-in");
    assert_eq!(alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204, "carol's attempt ended alice's sign-in");
    let (_, list) = alice.get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], true, "{list}");
}

/// The kill switch takes each sign-in out of the vault and revokes it at its
/// app before anything is recorded, so it works when the log does not. A
/// crash in between (the sign-in out of the vault, not yet revoked, the
/// switch not yet recorded) is finished at the next start: the vault comes
/// back sealed, the switch is recorded on, and the sign-in is revoked.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_switch_cut_short_by_a_crash_is_finished_at_the_next_start() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(steps(vec![fail("nothing to do")])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert!(st.is_success(), "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sign_in(&alice, "calendar", callback.local_addr().unwrap().port()).await;

    // What the vault holds after the switch took the sign-in out, then the
    // appliance died: sealed, the sign-in waiting to be revoked.
    app.kill().await;
    let vault = app.state_dir.join("vault");
    let mut all: serde_json::Map<String, Value> = serde_json::from_slice(&std::fs::read(vault.join("secrets.json")).unwrap()).unwrap();
    let entry = all.remove("connector_calendar__alice").expect("alice's sign-in is in the vault");
    std::fs::write(vault.join("revoking.json"), serde_json::to_vec(&json!({ "connector_calendar__alice": entry })).unwrap()).unwrap();
    std::fs::write(vault.join("secrets.json"), serde_json::to_vec(&all).unwrap()).unwrap();
    std::fs::write(vault.join("sealed"), b"").unwrap();
    app.restart().await;

    // Engaged before the appliance answers anything: until then, scheduled
    // and incoming work would run unchecked (review of #151).
    let alice = app.api("alice");
    let (_, first) = alice.get("/v1/kill-switch").await;
    assert_eq!(first["engaged"], true, "the appliance answered before the cut-short switch was engaged again: {first}");
    let ks = eventually("the cut-short switch is finished", PATIENCE, || async {
        let (_, ks) = alice.get("/v1/kill-switch").await;
        (ks["engaged"] == true && ks["revoked"]["connector_calendar__alice"]["at_provider"] == true).then_some(ks)
    })
    .await;
    assert!(cal.state.lock().unwrap().revoked.iter().any(|t| t.starts_with("rt-")), "{ks}");
    assert!(!vault.join("revoking.json").exists() || std::fs::read_to_string(vault.join("revoking.json")).unwrap().trim() == "{}", "a revoked sign-in is still waiting");
}

/// An approval covers the action as it was classed when given: the owner's
/// standing approval of a tool that cannot be undone does not cover the
/// same tool once the owner reclassifies it as one that moves money; that
/// call is asked again.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_does_not_cover_its_tool_once_it_is_classed_riskier() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("add the standup", "the standup is on the calendar"),
            1 => tool("calendar__create_event", json!({ "title": "Standup" })),
            _ => complete("added"),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert!(st.is_success(), "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sign_in(&alice, "calendar", callback.local_addr().unwrap().port()).await;
    assert_eq!(alice.put("/v1/connectors/calendar/tools/create_event", json!({ "risk": "irreversible" })).await.0, 204);

    let t = alice.submit("Put the standup on my calendar.").await;
    let item = alice.wait_item(&t, "approval").await;
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "standing" })).await.is_success());
    alice.wait_settled(&t).await;
    assert_eq!(cal.calls("create_event"), 1);

    // The same tool, now classed as one that moves money.
    assert_eq!(alice.put("/v1/connectors/calendar/tools/create_event", json!({ "risk": "financial" })).await.0, 204);
    let t = alice.submit("Put the standup on my calendar again.").await;
    let asked = eventually("the call is asked about or runs", PATIENCE, || {
        let (alice, t, cal) = (alice.clone(), t.clone(), &cal);
        async move {
            if cal.calls("create_event") > 1 {
                return Some(false);
            }
            (!alice.open_items(&t, "approval").await.is_empty()).then_some(true)
        }
    })
    .await;
    assert!(asked, "an approval given for an action that cannot be undone covered it once classed as one that moves money");
}

/// An approval keeps the class it was given at when the task that asked for
/// it is deleted: the task's approval item goes with its content, the
/// approval's record stays (review of #148).
#[tokio::test(flavor = "multi_thread")]
async fn a_standing_approval_outlives_deleting_the_task_that_asked_for_it() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("add the standup", "the standup is on the calendar"),
            1 => tool("calendar__create_event", json!({ "title": "Standup" })),
            _ => complete("added"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert!(st.is_success(), "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sign_in(&alice, "calendar", callback.local_addr().unwrap().port()).await;
    assert_eq!(alice.put("/v1/connectors/calendar/tools/create_event", json!({ "risk": "irreversible" })).await.0, 204);

    let t = alice.submit("Put the standup on my calendar.").await;
    let item = alice.wait_item(&t, "approval").await;
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "standing" })).await.is_success());
    alice.wait_settled(&t).await;
    assert_eq!(cal.calls("create_event"), 1);
    assert!(alice.post(&format!("/v1/tasks/{t}/cancel"), json!({})).await.0.is_success());
    alice.wait_status(&t, &["cancelled"]).await;
    let (st, v) = alice.post("/v1/trash", json!({ "item": t })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(alice.post("/v1/trash/empty", json!({})).await.0, 200);
    app.restart().await;

    let t = app.api("alice").submit("Put the standup on my calendar again.").await;
    let ran = eventually("the call runs or is asked about", PATIENCE, || {
        let (alice, t, cal) = (app.api("alice"), t.clone(), &cal);
        async move {
            if cal.calls("create_event") > 1 {
                return Some(true);
            }
            (!alice.open_items(&t, "approval").await.is_empty()).then_some(false)
        }
    })
    .await;
    assert!(ran, "deleting the task that asked for a standing approval revoked it");
}

/// An app's address carries no key or token (connectors spec: nobody types
/// one into Magnus): one in its query or user part is refused before the
/// app is asked anything, and never reaches the log or the app list.
#[tokio::test(flavor = "multi_thread")]
async fn an_app_address_holding_a_secret_is_refused() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = start_checked(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let with_query = format!("{}?api_key=s3cret-in-query", cal.url);
    let with_user = cal.url.replacen("://", "://me:s3cret-in-user@", 1);
    for url in [&with_query, &with_user] {
        let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": url, "audience": "private", "label": "private+untrusted" })).await;
        assert_eq!(st, 400, "an address holding a secret was taken: {url} {v}");
    }
    let (_, list) = alice.get("/v1/connectors").await;
    assert!(!list.to_string().contains("s3cret"), "{list}");
    let (_, audit) = alice.get("/v1/audit").await;
    assert!(!audit.to_string().contains("s3cret"), "the secret reached the log");
    // The address itself is still taken.
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
}

/// `sandbox` is the appliance's own microVM, never an app: an app's tool
/// classed so would skip the task contract, the Rule of Two and the
/// audience rule, as if what it did stayed on the appliance. The owner
/// cannot class one so, and one an older log classed so is not offered.
#[tokio::test(flavor = "multi_thread")]
async fn an_apps_tool_is_never_classed_as_the_sandbox() {
    let cal = FakeOAuthApp::start(3600).await;
    // No contract: the call is refused unless its class skips the contract.
    let m = start_checked(script(|c| async move {
        match c.turn() {
            0 => tool("calendar__create_event", json!({ "title": "Standup" })),
            _ => fail("not offered"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert!(st.is_success(), "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sign_in(&alice, "calendar", callback.local_addr().unwrap().port()).await;
    assert_eq!(alice.put("/v1/connectors/calendar/tools/create_event", json!({ "risk": "sandbox" })).await.0, 400, "an app's tool was classed as the sandbox");

    // An older release took it.
    app.kill().await;
    let admin = app.db.admin().await;
    let ev = json!({ "kind": "tool_classified", "connector": "calendar", "tool": "create_event", "risk": "sandbox" });
    admin.execute("INSERT INTO public.events (kind, actor, run_id, body) VALUES ('tool_classified', 'person:alice', NULL, $1)", &[&ev]).await.unwrap();
    app.restart().await;
    let alice = app.api("alice");
    let t = alice.submit("Put the standup on my calendar.").await;
    alice.wait_settled(&t).await;
    assert_eq!(cal.calls("create_event"), 0, "an app's tool classed as the sandbox ran without a contract");
    let offered: Vec<String> = m.requests().iter().flat_map(|c| c.offered_tools()).collect();
    assert!(!offered.iter().any(|t| t == "calendar__create_event"), "{offered:?}");
}
