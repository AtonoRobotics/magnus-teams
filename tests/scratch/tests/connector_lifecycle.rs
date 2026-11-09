//! A person's connection to an app, from connecting to Disconnect
//! (connectors spec, Lifecycle; Org policy, isolation and audit):
//! - connecting and disconnecting are each in the log, without a token;
//! - Disconnect first revokes the sign-in at the app, its refresh and access
//!   tokens both, then deletes it here, and says the app ended it;
//! - an app with no revocation endpoint is disconnected here all the same,
//!   and the person is told the app could not end it itself;
//! - a sign-in the person ended in the app's own settings makes the next
//!   use fail, and the card asks them to sign in again (Needs attention).

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

/// Alice signs in to the app with her own account.
async fn connect(api: &Api, app: &str) {
    let (st, v) = api.post(&format!("/v1/connectors/{app}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(api.post(&format!("/v1/connectors/{app}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
}

async fn add(alice: &Api, url: &str) -> String {
    let (st, v) = alice.post("/v1/connectors", json!({ "url": url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    v["name"].as_str().unwrap().to_string()
}

async fn card(api: &Api, app: &str) -> Value {
    let (st, v) = api.get(&format!("/v1/connectors/{app}")).await;
    assert!(st.is_success(), "{v}");
    v
}

async fn logged(app: &Appliance, kind: &str) -> Vec<Value> {
    let db = app.db.admin().await;
    db.query("SELECT body FROM public.events WHERE kind = $1 ORDER BY seq", &[&kind]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnect_revokes_at_the_app_first_and_both_ends_of_a_connection_are_in_the_log() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let name = add(&alice, &cal.url).await;
    connect(&alice, &name).await;
    assert_eq!(card(&alice, &name).await["state"], "connected");
    let issued: Vec<String> = { cal.state.lock().unwrap().revoked.clone() };
    assert!(issued.is_empty());

    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/connectors/{name}/connection"), None).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert_eq!(v["at_provider"], true, "the app did not end the sign-in: {v}");
    let revoked = cal.state.lock().unwrap().revoked.clone();
    assert!(revoked.iter().any(|t| t.starts_with("rt-")) && revoked.iter().any(|t| !t.starts_with("rt-")), "both tokens were not revoked at the app: {revoked:?}");
    assert_eq!(card(&alice, &name).await["state"], "available");
    let (st, _) = alice.send(reqwest::Method::DELETE, &format!("/v1/connectors/{name}/connection"), None).await;
    assert_eq!(st.as_u16(), 404, "a connection was ended twice");

    // Both ends are in the log, each with whose it was, and no token.
    let (on, off) = (logged(&app, "app_connected").await, logged(&app, "app_disconnected").await);
    assert_eq!((on.len(), off.len()), (1, 1), "{on:?} {off:?}");
    assert_eq!((on[0]["app"].as_str(), on[0]["person"].as_str()), (Some(name.as_str()), Some("alice")), "{}", on[0]);
    assert_eq!((off[0]["person"].as_str(), off[0]["at_provider"].as_bool()), (Some("alice"), Some(true)), "{}", off[0]);
    let all = format!("{on:?}{off:?}");
    assert!(revoked.iter().all(|t| !all.contains(t.as_str())), "a token is in the log");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_that_cannot_revoke_is_disconnected_here_and_the_person_is_told() {
    let cal = FakeOAuthApp::start(3600).await;
    cal.state.lock().unwrap().no_revocation = true;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let name = add(&alice, &cal.url).await;
    connect(&alice, &name).await;

    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/connectors/{name}/connection"), None).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert_eq!(v["at_provider"], false, "{v}");
    assert!(v["detail"].as_str().is_some_and(|d| d.contains("no revocation endpoint")), "the person is not told why: {v}");
    assert_eq!(card(&alice, &name).await["state"], "available", "the sign-in was kept because the app could not revoke it");
    let off = logged(&app, "app_disconnected").await;
    assert_eq!(off.last().unwrap()["at_provider"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_ended_at_the_app_asks_the_person_to_sign_in_again() {
    // Tokens last 30 s, inside the minute Magnus refreshes ahead: every use refreshes.
    let cal = FakeOAuthApp::start(30).await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let name = add(&alice, &cal.url).await;
    connect(&alice, &name).await;
    assert_eq!(card(&alice, &name).await["state"], "connected");

    // Alice removes Magnus in the app's own settings.
    cal.revoke_everything();
    let c = card(&alice, &name).await;
    assert_eq!((c["state"].as_str(), c["signed_in"].as_bool()), (Some("needs_attention"), Some(false)), "{c}");
    // Signing in again brings it back.
    connect(&alice, &name).await;
    assert_eq!(card(&alice, &name).await["state"], "connected");
}

/// An app the owner turned off keeps its people's sign-ins (turned on again,
/// they work), so they can still be ended: a person's Disconnect revokes
/// theirs at the app, and the kill switch revokes every one (B11).
#[tokio::test(flavor = "multi_thread")]
async fn an_app_turned_off_still_has_its_sign_ins_ended_by_disconnect_and_the_kill_switch() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let name = add(&alice, &cal.url).await;
    connect(&alice, &name).await;
    connect(&carol, &name).await;
    let (st, v) = control(&app, "PATCH", &format!("/v1/admin/connectors/{name}"), json!({ "actor": "admin:admin-1", "enabled": false })).await;
    assert_eq!(st, 200, "{v}");

    // Alice ends her own sign-in to it, at the app.
    let (st, ended) = alice.send(reqwest::Method::DELETE, &format!("/v1/connectors/{name}/connection"), None).await;
    assert_eq!((st.as_u16(), ended["at_provider"].as_bool()), (200, Some(true)), "alice could not end her sign-in to a turned-off app: {ended}");

    // The kill switch revokes Carol's at the app.
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    eventually("carol's sign-in to the turned-off app is revoked", PATIENCE, || async {
        let (_, ks) = alice.get("/v1/kill-switch").await;
        (ks["revoked"][format!("connector_{name}__carol")]["at_provider"] == true).then_some(ks)
    })
    .await;
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
}

/// The app's authorization server answers the first registration with 429,
/// as a provider does under load: the sign-in waits and registers anyway,
/// once (#315).
#[tokio::test(flavor = "multi_thread")]
async fn a_throttled_first_registration_is_retried_and_the_sign_in_succeeds() {
    let cal = FakeOAuthApp::start(3600).await;
    cal.state.lock().unwrap().throttle_registrations = 1;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let name = add(&alice, &cal.url).await;
    connect(&alice, &name).await;
    assert_eq!(card(&alice, &name).await["state"], "connected");
    assert_eq!(cal.state.lock().unwrap().registrations, 1, "the 429 registered, or the retry did not");
}
