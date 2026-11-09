//! Users in the admin console (admin console spec, Users; invariant "Removing,
//! disabling or demoting the last admin is refused"), against the local
//! store through the kernel's own endpoints: an admin invites someone, the
//! console shows the single-use token to pass on (the appliance sends no
//! mail), they set their password with it and are active; a role changes
//! only with a fresh password; the last admin can be neither demoted nor
//! turned off; a person turned off loses every session here, and their
//! schedules pause; each change is in the audit log.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::{json, Value};

struct Setup {
    app: Appliance,
    url: String,
    c: Console,
    b: reqwest::Client,
}

impl Setup {
    async fn start(step_up_seconds: i64) -> Setup {
        Setup::start_with(step_up_seconds, &[]).await
    }

    /// With more of the console's settings (`refresh_seconds`, say).
    async fn start_with(step_up_seconds: i64, more: &[(&str, toml::Value)]) -> Setup {
        let m = FakeModel::start(steps(vec![])).await;
        let app = Appliance::start(Spec::default(), &m).await;
        local_user(&app, "Alice", PASSWORD, true);
        let mine = format!("{}/32", lan_ip());
        let mut settings = vec![("step_up_seconds", step_up_seconds.into())];
        settings.extend(more.iter().cloned());
        let p = prepare(&app, &[&mine], &settings);
        let url = p.url();
        let c = launch(p).await;
        let b = browser(&c, false);
        Setup { app, url, c, b }
    }

    fn api(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.url)
    }

    /// Alice's console session and its CSRF token, stepped up if asked.
    /// The tests' step-up windows (60 seconds) are long enough for the
    /// browser to hand the session back (that has taken 23 seconds after
    /// the password on a loaded machine).
    async fn alice(&self, step_up: bool) -> (String, String) {
        let sid = sign_in(&self.url, self.c.addr, "Alice", PASSWORD, "admin", step_up).await.unwrap_or_else(|e| panic!("{e}"));
        let me: Value = with_cookie(self.b.get(self.api("/session")), &sid).send().await.unwrap().json().await.unwrap();
        (sid, me["csrf"].as_str().unwrap().to_string())
    }

    async fn send(&self, method: reqwest::Method, path: &str, s: &(String, String), body: Option<Value>) -> (StatusCode, Value) {
        let mut rq = with_cookie(self.b.request(method, self.api(path)), &s.0).header("x-csrf-token", &s.1);
        if let Some(b) = body {
            rq = rq.json(&b);
        }
        let r = rq.send().await.unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    async fn users(&self, s: &(String, String)) -> Vec<Value> {
        let (st, v) = self.send(reqwest::Method::GET, "/users", s, None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["users"].as_array().unwrap().clone()
    }

    async fn user(&self, s: &(String, String), id: &str) -> Option<Value> {
        self.users(s).await.into_iter().find(|u| u["id"] == id)
    }

    async fn audited(&self, s: &(String, String)) -> Vec<String> {
        let (_, v) = self.send(reqwest::Method::GET, "/audit", s, None).await;
        v["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str().map(String::from)).collect()
    }

    /// Someone's password at the app's own sign-in.
    async fn app_sign_in(&self, person: &str, password: &str) -> (StatusCode, Value) {
        let r = reqwest::Client::new()
            .post(format!("{}/v1/sign-in", self.app.base))
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "person": person, "password": password, "client": "app" }))
            .send()
            .await
            .unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_invites_someone_who_sets_their_password_with_the_shown_token() {
    let s = Setup::start(60).await;
    let alice = s.alice(false).await;

    let me = s.user(&alice, "alice").await.expect("Alice is not in the list");
    assert_eq!((me["role"].as_str(), me["status"].as_str()), (Some("admin"), Some("active")), "{me}");

    // A member is invited without a step-up: the account waits for its
    // password, and the console shows the single-use token to pass on —
    // the appliance sends no mail, so no email is ever sent.
    let (st, v) = s.send(reqwest::Method::POST, "/users", &alice, Some(json!({ "email": "Dee@Acme.test", "name": "Dee Ray", "role": "member" }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!((v["id"].as_str(), v["email"].as_str(), v["email_sent"].as_bool()), (Some("dee"), Some("dee@acme.test"), Some(false)), "{v}");
    assert!(v["invite_token"].as_str().is_some_and(|t| t.len() >= 16) && v["invite_expires"].is_string(), "{v}");
    let first_token = v["invite_token"].as_str().unwrap().to_string();
    let dee = s.user(&alice, "dee").await.expect("the invitation is not in the list");
    assert_eq!(
        (dee["name"].as_str(), dee["email"].as_str(), dee["role"].as_str(), dee["status"].as_str()),
        (Some("Dee Ray"), Some("dee@acme.test"), Some("member"), Some("invited")),
        "{dee}"
    );
    // Nobody signs in before choosing a password.
    let (st, _) = s.app_sign_in("dee", "whatever-password").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // The id is held: inviting again refuses, never a surprising sibling.
    let (st, v) = s.send(reqwest::Method::POST, "/users", &alice, Some(json!({ "email": "dee@acme.test", "name": "Dee Two", "role": "member" }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("taken")), "{v}");

    // Resend mints a fresh token and kills the old one.
    let (st, v) = s.send(reqwest::Method::POST, "/users/dee/invite", &alice, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let token = v["invite_token"].as_str().unwrap().to_string();
    assert_ne!(token, first_token, "resending showed the same token");
    let http = reqwest::Client::new();
    let r = http.post(format!("{}/v1/invites/accept", s.app.base)).json(&json!({ "token": first_token, "password": "dee-long-password" })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the replaced token still accepts");

    // Dee sets a password with the token, and is an active member who can
    // sign in; the token works once.
    let r = http.post(format!("{}/v1/invites/accept", s.app.base)).json(&json!({ "token": token, "password": "dee-long-password" })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "{}", r.text().await.unwrap());
    let r = http.post(format!("{}/v1/invites/accept", s.app.base)).json(&json!({ "token": token, "password": "dee-other-password" })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the token accepted twice");
    let dee = s.user(&alice, "dee").await.unwrap();
    assert_eq!(dee["status"], "active", "{dee}");
    let (st, v) = s.app_sign_in("dee", "dee-long-password").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // An account in use is no invitation: nothing to resend or cancel.
    let (st, v) = s.send(reqwest::Method::DELETE, "/users/dee/invite", &alice, None).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("not_invited")), "{v}");

    // Inviting an admin needs a fresh password: refused once the sign-in's
    // own step-up has lapsed, done with a fresh one.
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    let eve = json!({ "email": "eve@acme.test", "name": "Eve Ito", "role": "admin" });
    let (st, v) = s.send(reqwest::Method::POST, "/users", &alice, Some(eve.clone())).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{v}");
    assert!(s.user(&alice, "eve").await.is_none(), "an admin was invited without a fresh password");
    let fresh = s.alice(true).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users", &fresh, Some(eve)).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let eve = s.user(&fresh, "eve").await.unwrap();
    assert_eq!((eve["role"].as_str(), eve["status"].as_str()), (Some("admin"), Some("invited")), "{eve}");
    // Cancelled, the invitation and its account are gone.
    let (st, v) = s.send(reqwest::Method::DELETE, "/users/eve/invite", &fresh, None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    assert!(s.user(&fresh, "eve").await.is_none(), "the cancelled invitation is still there");

    // Bad invitations change nothing.
    for bad in [
        json!({ "email": "not-an-address", "name": "X", "role": "member" }),
        json!({ "email": "x@acme.test", "name": "", "role": "member" }),
        json!({ "email": "x@acme.test", "name": "X", "role": "owner" }),
    ] {
        let (st, _) = s.send(reqwest::Method::POST, "/users", &fresh, Some(bad.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
    }

    // The list renders in every look, and at phone width with no sideways scroll.
    sign_in_and_visit(&s.url, s.c.addr, "Alice", PASSWORD, "admin", false, Some("#/users")).await.unwrap_or_else(|e| panic!("{e}"));

    let actions = s.audited(&fresh).await;
    for a in ["user_invited", "user_invite_resent", "user_invite_cancelled"] {
        assert!(actions.iter().any(|x| x == a), "{a} is not in the audit: {actions:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_link_an_admin_gives_an_invited_person_a_one_time_password() {
    // No invite link can reach them: the admin passes on a one-time
    // password instead, which chooses their own and never signs in.
    let s = Setup::start(60).await;
    let alice = s.alice(false).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users", &alice, Some(json!({ "email": "dee@acme.test", "name": "Dee Ray", "role": "member" }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");

    let (st, v) = s.send(reqwest::Method::POST, "/users/dee/invite/password", &alice, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let password = v["password"].as_str().unwrap().to_string();
    assert!(password.len() >= 20 && password.chars().all(|c| c.is_ascii_alphanumeric()), "{v}");
    assert_eq!(v["username"], "dee", "{v}");
    // Still an invitation: the temp never signs in, it only chooses.
    let (st, v) = s.app_sign_in("dee", &password).await;
    assert_eq!((st, v["must_choose"].as_bool()), (StatusCode::UNAUTHORIZED, Some(true)), "{v}");
    assert_eq!(s.user(&alice, "dee").await.unwrap()["status"], "invited");
    assert!(s.audited(&alice).await.iter().any(|a| a == "user_invite_password_set"), "not in the audit log");

    // Dee chooses with it, and is an active member; the temp is spent.
    let http = reqwest::Client::new();
    let r = http.post(format!("{}/v1/password", s.app.base)).json(&json!({ "person": "dee", "password": password, "new_password": "dee-long-password" })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "{}", r.text().await.unwrap());
    assert_eq!(s.user(&alice, "dee").await.unwrap()["status"], "active");
    let (st, v) = s.app_sign_in("dee", "dee-long-password").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // An account in use gets no password from here.
    let (st, v) = s.send(reqwest::Method::POST, "/users/dee/invite/password", &alice, None).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("not_invited")), "{v}");

    // An admin's invitation needs a fresh password, as inviting them does.
    let fresh = s.alice(true).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users", &fresh, Some(json!({ "email": "eve@acme.test", "name": "Eve Ito", "role": "admin" }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users/eve/invite/password", &fresh, None).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{v}");
    let fresh = s.alice(true).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users/eve/invite/password", &fresh, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_last_admin_stays_and_a_person_turned_off_loses_every_session_and_schedule() {
    let s = Setup::start(60).await;
    local_user(&s.app, "Bob", PASSWORD, false);

    // Bob signs in to the app and sets up a schedule.
    let (st, v) = s.app_sign_in("bob", PASSWORD).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let bob_token = v["token"].as_str().unwrap().to_string();
    let http = reqwest::Client::new();
    let r = http.post(format!("{}/v1/schedules", s.app.base)).bearer_auth(&bob_token).json(&json!({ "text": "Water the plants.", "daily_at": "03:00" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());

    let alice = s.alice(true).await;
    let bob = s.user(&alice, "bob").await.expect("Bob is not in the list");
    assert_eq!((bob["role"].as_str(), bob["status"].as_str()), (Some("member"), Some("active")), "{bob}");
    assert!(bob["last_sign_in"].is_string() && bob["sessions"] == 1, "Bob's sign-in here is not shown: {bob}");
    let (st, detail) = s.send(reqwest::Method::GET, "/users/bob", &alice, None).await;
    assert_eq!(st, StatusCode::OK, "{detail}");
    assert_eq!(detail["sessions"].as_array().map(Vec::len), Some(1), "{detail}");

    // Alice is the last admin: she can be neither turned off nor demoted.
    let (st, v) = s.send(reqwest::Method::POST, "/users/alice/disable", &alice, None).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("last_admin")), "{v}");
    let (st, v) = s.send(reqwest::Method::PATCH, "/users/alice", &alice, Some(json!({ "role": "member" }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("last_admin")), "{v}");
    let (st, _) = s.app_sign_in("alice", PASSWORD).await;
    assert_eq!(st, StatusCode::OK, "the last admin was turned off");

    // With a fresh password, Bob is made an admin, then a member again.
    let (st, v) = s.send(reqwest::Method::PATCH, "/users/bob", &alice, Some(json!({ "role": "admin" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(s.user(&alice, "bob").await.unwrap()["role"], "admin");
    let (st, v) = s.send(reqwest::Method::PATCH, "/users/bob", &alice, Some(json!({ "role": "member" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(s.user(&alice, "bob").await.unwrap()["role"], "member");

    // Turned off: here refuses him, his app session is over, and his
    // schedule is paused.
    let (st, v) = s.send(reqwest::Method::POST, "/users/bob/disable", &alice, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!((v["sessions_ended"].as_u64(), v["schedules_paused"].as_u64()), (Some(1), Some(1)), "{v}");
    assert_eq!(s.user(&alice, "bob").await.unwrap()["status"], "disabled");
    let (st, _) = s.app_sign_in("bob", PASSWORD).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "here still signs in a person turned off");
    let r = http.get(format!("{}/v1/schedules", s.app.base)).bearer_auth(&bob_token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "a session of a person turned off still works");
    let (_, sc) = s.app.api("bob").get("/v1/schedules").await;
    assert!(sc["schedules"][0]["paused"].is_string(), "the schedule of a person turned off still runs: {sc}");

    // Turned back on, he signs in again; ending his sessions signs him out.
    let (st, v) = s.send(reqwest::Method::POST, "/users/bob/enable", &alice, None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(s.user(&alice, "bob").await.unwrap()["status"], "active");
    let (st, v) = s.app_sign_in("bob", PASSWORD).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let bob_token = v["token"].as_str().unwrap().to_string();
    let (st, v) = s.send(reqwest::Method::DELETE, "/users/bob/sessions", &alice, None).await;
    assert_eq!((st, v["ended"].as_u64()), (StatusCode::OK, Some(1)), "{v}");
    let r = http.get(format!("{}/v1/schedules", s.app.base)).bearer_auth(&bob_token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "an ended session still works");
    // One session, by its id.
    let (_, v) = s.app_sign_in("bob", PASSWORD).await;
    let bob_token = v["token"].as_str().unwrap().to_string();
    let (_, detail) = s.send(reqwest::Method::GET, "/users/bob", &alice, None).await;
    let sid = detail["sessions"][0]["id"].as_str().unwrap_or_else(|| panic!("{detail}")).to_string();
    let (st, v) = s.send(reqwest::Method::DELETE, &format!("/users/bob/sessions/{sid}"), &alice, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let r = http.get(format!("{}/v1/schedules", s.app.base)).bearer_auth(&bob_token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "the ended session still works");

    // Once the step-up lapses, a role change says so and changes nothing.
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    let (st, v) = s.send(reqwest::Method::PATCH, "/users/bob", &alice, Some(json!({ "role": "admin" }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{v}");
    assert_eq!(s.user(&alice, "bob").await.unwrap()["role"], "member");

    // A person's page renders in every look, and at phone width.
    sign_in_and_visit(&s.url, s.c.addr, "Alice", PASSWORD, "admin", false, Some("#/users/bob/sessions")).await.unwrap_or_else(|e| panic!("{e}"));

    let actions = s.audited(&alice).await;
    for a in ["user_role_changed", "user_disabled", "user_enabled", "user_sessions_ended", "user_session_ended"] {
        assert!(actions.iter().any(|x| x == a), "{a} is not in the audit: {actions:?}");
    }
}

/// Deleting the last admin is refused before anything is erased: their
/// account, sign-in and content are all untouched (#527).
#[tokio::test(flavor = "multi_thread")]
async fn deleting_the_last_admin_is_refused_and_erases_nothing() {
    // Alice's console session outlives her losing the role, so she can
    // still aim a delete at Bob once he is the only admin left.
    let s = Setup::start_with(600, &[("refresh_seconds", 3600.into())]).await;
    local_user(&s.app, "Bob", PASSWORD, true);
    let (_, v) = s.app.api("bob").post("/v1/schedules", json!({ "text": "Water the plants.", "daily_at": "03:00" })).await;
    assert!(v["id"].is_string(), "{v}");
    let alice = s.alice(true).await;
    let (st, v) = s.send(reqwest::Method::PATCH, "/users/alice", &alice, Some(json!({ "role": "member" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    // Bob has no email address, so the typed confirmation is his id.
    let (st, v) = s.send(reqwest::Method::DELETE, "/users/bob", &alice, Some(json!({ "email": "bob" }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("last_admin")), "{v}");
    let bob = s.user(&alice, "bob").await.expect("Bob is gone from the list");
    assert_eq!((bob["role"].as_str(), bob["status"].as_str()), (Some("admin"), Some("active")), "{bob}");
    let (st, _) = s.app_sign_in("bob", PASSWORD).await;
    assert_eq!(st, StatusCode::OK, "the last admin cannot sign in after the refused delete");
    let (_, sc) = s.app.api("bob").get("/v1/schedules").await;
    assert_eq!(sc["schedules"].as_array().map(Vec::len), Some(1), "the refused delete erased his schedule: {sc}");
}

/// Deleting a local person erases their data and then removes their
/// account: they leave the list, their sessions and schedules are
/// counted, and nothing of theirs signs in (#529).
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_local_person_erases_then_removes_them() {
    let s = Setup::start(60).await;
    let alice = s.alice(true).await;
    let (st, v) = s.send(reqwest::Method::POST, "/users", &alice, Some(json!({ "email": "dee@acme.test", "name": "Dee Ray", "role": "member" }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let token = v["invite_token"].as_str().unwrap().to_string();
    let http = reqwest::Client::new();
    let r = http.post(format!("{}/v1/invites/accept", s.app.base)).json(&json!({ "token": token, "password": "dee-long-password" })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "{}", r.text().await.unwrap());
    let (st, v) = s.app_sign_in("dee", "dee-long-password").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let dee_token = v["token"].as_str().unwrap().to_string();
    let r = http.post(format!("{}/v1/schedules", s.app.base)).bearer_auth(&dee_token).json(&json!({ "text": "Water the plants.", "daily_at": "03:00" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());

    let (st, v) = s.send(reqwest::Method::DELETE, "/users/dee", &alice, Some(json!({ "email": "dee@acme.test" }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["account_removed"], true, "{v}");
    assert_eq!((v["erased"]["schedules_ended"].as_u64(), v["erased"]["sessions_ended"].as_u64()), (Some(1), Some(1)), "{v}");
    assert!(s.user(&alice, "dee").await.is_none(), "the deleted person is still listed");
    let (st, _) = s.app_sign_in("dee", "dee-long-password").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "the deleted person still signs in");
    let r = http.get(format!("{}/v1/schedules", s.app.base)).bearer_auth(&dee_token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "the deleted person's session still works");
    assert!(s.audited(&alice).await.iter().any(|a| a == "user_deleted"), "not in the audit log");
}

/// Issue #192: two changes aimed at the last two admins, sent together,
/// each demoting or turning off one of them. The console lets one through
/// and refuses the other, whatever the timing: at least one admin turned on
/// is left. Each round sends both at the same moment (a barrier); the
/// rounds cover every pairing of demote and turn off.
#[tokio::test(flavor = "multi_thread")]
async fn two_changes_at_once_never_take_away_the_last_admin() {
    // Alice's console session outlives her losing the role in a round.
    let s = Setup::start_with(600, &[("refresh_seconds", 3600.into())]).await;
    local_user(&s.app, "Bob", PASSWORD, true);
    let alice = s.alice(true).await;
    let admins_on = |list: &[Value]| list.iter().filter(|u| u["role"] == "admin" && u["status"] == "active").count();
    let demote = |id: &str| (reqwest::Method::PATCH, format!("/users/{id}"), Some(json!({ "role": "member" })));
    let turn_off = |id: &str| (reqwest::Method::POST, format!("/users/{id}/disable"), None);
    let pairs = [
        ("demote both", demote("alice"), demote("bob")),
        ("turn both off", turn_off("alice"), turn_off("bob")),
        ("demote Alice, turn Bob off", demote("alice"), turn_off("bob")),
        ("turn Alice off, demote Bob", turn_off("alice"), demote("bob")),
    ];
    for round in 0..12 {
        let (what, one, two) = pairs[round % pairs.len()].clone();
        // Both are admins and turned on again, through the console itself.
        for id in ["alice", "bob"] {
            let (st, _) = s.send(reqwest::Method::PATCH, &format!("/users/{id}"), &alice, Some(json!({ "role": "admin" }))).await;
            assert!(st.is_success(), "round {round}: {id} is not an admin again: {st}");
            let (st, _) = s.send(reqwest::Method::POST, &format!("/users/{id}/enable"), &alice, None).await;
            assert!(st.is_success(), "round {round}: {id} is not turned on again: {st}");
        }
        assert_eq!(admins_on(&s.users(&alice).await), 2, "round {round}: two admins to start with");
        let gate = tokio::sync::Barrier::new(2);
        let go = |(method, path, body): (reqwest::Method, String, Option<Value>)| {
            let (gate, alice, s) = (&gate, &alice, &s);
            async move {
                gate.wait().await;
                s.send(method, &path, alice, body).await
            }
        };
        let (r1, r2) = tokio::join!(go(one), go(two));
        let left = admins_on(&s.users(&alice).await);
        assert!(left >= 1, "round {round} ({what}): both changes went through and no admin is left: {r1:?} {r2:?}");
        let refused = [&r1, &r2].iter().filter(|(st, v)| *st == StatusCode::CONFLICT && v["error"] == "last_admin").count();
        let done = [&r1, &r2].iter().filter(|(st, _)| st.is_success()).count();
        assert_eq!((done, refused), (1, 1), "round {round} ({what}): one change goes through and the other is refused: {r1:?} {r2:?}");
    }
}
