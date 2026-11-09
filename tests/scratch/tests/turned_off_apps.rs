//! A person turned off at the identity provider has no apps until they are
//! back (connectors spec, Isolation: "their connections refuse every call
//! and schedules pause until they are back; tokens stay in the vault
//! unused"), against a real Keycloak. Carol's task waits on an approval to
//! read her calendar; Keycloak turns her off; approved after that, the call
//! is refused and never reaches the app, and her sign-in to it is kept, not
//! revoked. Turned back on and signed in again, her next task's call goes
//! through with the same sign-in.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

/// Carol, signed in to the app through Keycloak.
async fn signed_in(kc: &Keycloak, app: &Appliance) -> Api {
    let (st, v) = kc.app_sign_in(app, "carol").await;
    assert_eq!(st, 200, "{v}");
    let mut carol = app.api("carol");
    carol.token = v["token"].as_str().unwrap().to_string();
    carol
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_turned_off_has_no_apps_until_they_are_back() {
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("list the events", "the events are listed"),
            1 => tool("calendar__list_events", json!({})),
            _ => complete("done"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    kc.add_person("carol").await;
    kc.configure(&mut app).await;
    let cal = FakeOAuthApp::start(3600).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    // Every read of it asks first, so a task waits with the call ahead of it.
    assert_eq!(alice.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "irreversible" })).await.0, 204);

    let carol = signed_in(&kc, &app).await;
    let (st, v) = carol.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(carol.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);

    let task = carol.submit("What is on my calendar?").await;
    let item = carol.wait_item(&task, "approval").await;

    // Keycloak turns her off; the check runs as the appliance starts.
    kc.disable("carol").await;
    app.restart().await;
    eventually("carol's session ended", PATIENCE, || async { (carol.get("/v1/me").await.0 == 401).then_some(()) }).await;

    // Approved now, the call is refused here and never reaches the app.
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    let refused = eventually("the call refused", PATIENCE, || async { m.requests().iter().map(|c| c.text()).find(|t| t.contains("account is turned off")) }).await;
    assert!(refused.contains("Nothing was sent to calendar"), "{refused}");
    assert_eq!(cal.calls("list_events"), 0, "a turned-off person's app was called");
    // Her sign-in is kept, unused: nothing was revoked.
    assert!(cal.state.lock().unwrap().revoked.is_empty(), "turning her off revoked her sign-in");

    // Back on, and signed in again: the same sign-in works.
    kc.enable("carol").await;
    let carol = signed_in(&kc, &app).await;
    let (_, c) = carol.get("/v1/connectors/calendar").await;
    assert_eq!(c["state"], "connected", "{c}");
    let task = carol.submit("What is on my calendar now?").await;
    let item = carol.wait_item(&task, "approval").await;
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("the call made", PATIENCE, || async { (cal.calls("list_events") == 1).then_some(()) }).await;
}
