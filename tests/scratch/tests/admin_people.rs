//! Users › Schedules and triggers, Data and Delete in the admin console (admin
//! console spec; invariant "An admin reading a user's private data succeeds,
//! and the read appears in the audit log with admin, target, resource and
//! time"; owner's ruling 14), against the local store and a real appliance:
//! an admin suspends (which the person cannot lift) and resumes a person's schedule; reads their files, and
//! one file's words, each read in the audit log and in the person's inbox,
//! and refused while the organisation keeps people's data private; deletes
//! them, only with a fresh password and their email typed, after which their
//! account, sessions, schedules and content are gone, and their sign-ins to
//! apps revoked at each app.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const WORDS: &str = "the dock contract renews on the ninth of March";

struct Setup {
    url: String,
    c: Console,
    b: reqwest::Client,
}

impl Setup {
    async fn session(&self) -> (String, String) {
        let sid = sign_in(&self.url, self.c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
        let me: Value = with_cookie(self.b.get(format!("{}/api/v1/session", self.url)), &sid).send().await.unwrap().json().await.unwrap();
        (sid, me["csrf"].as_str().unwrap().to_string())
    }

    async fn send(&self, s: &(String, String), method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut rq = with_cookie(self.b.request(method, format!("{}/api/v1{path}", self.url)), &s.0).header("x-csrf-token", &s.1);
        if let Some(b) = body {
            rq = rq.json(&b);
        }
        let r = rq.send().await.unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn root_suspends_a_persons_schedule_reads_their_data_on_the_record_and_deletes_them_for_good() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    local_user(&app, "Bob", PASSWORD, false);
    let mine = format!("{}/32", lan_ip());
    // Long enough for a browser's sign-in to hand back its session (that
    // has taken 23 seconds after the password on a loaded machine).
    let lapse: i64 = 60;
    let p = prepare(&app, &[&mine], &[("step_up_seconds", lapse.into())]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let s = Setup { url: url.clone(), c, b };

    // Bob signs in to the app, keeps a file and sets up a schedule.
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": "bob", "password": PASSWORD, "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    let bob_token = v["token"].as_str().unwrap().to_string();
    let http = reqwest::Client::new();
    let bob = |m: Method, path: &str| http.request(m, format!("{}{path}", app.base)).bearer_auth(&bob_token);
    use base64::Engine;
    let r = bob(Method::POST, "/v1/files")
        .json(&json!({ "name": "contracts.txt", "media_type": "text/plain", "data_b64": base64::engine::general_purpose::STANDARD.encode(WORDS) }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());
    let r = bob(Method::POST, "/v1/schedules").json(&json!({ "text": "Check the dock contracts.", "daily_at": "03:00" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());
    // And connects the organisation's calendar with his own sign-in.
    let cal = FakeOAuthApp::start(3600).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let r: Value = bob(Method::POST, "/v1/connectors/calendar/sign-in").json(&json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).send().await.unwrap().json().await.unwrap();
    let (code, state) = FakeOAuthApp::browse(r["authorize_url"].as_str().unwrap_or_else(|| panic!("{r}"))).await;
    let r = bob(Method::POST, "/v1/connectors/calendar/sign-in/complete").json(&json!({ "state": state, "code": code })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204, "{}", r.text().await.unwrap());

    let alice = s.session().await;
    let (_, users) = s.send(&alice, Method::GET, "/users", None).await;
    let b_id = users["users"].as_array().unwrap().iter().find(|u| u["id"] == "bob").unwrap()["id"].as_str().unwrap().to_string();

    // Schedules: Bob's one, suspended by the admin and resumed.
    let (st, v) = s.send(&alice, Method::GET, &format!("/users/{b_id}/schedules"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sched = v["schedules"].as_array().unwrap();
    assert_eq!(sched.len(), 1, "{v}");
    assert_eq!(sched[0]["title"], "Check the dock contracts.", "{v}");
    let sid = sched[0]["id"].as_str().unwrap().to_string();
    let (st, v) = s.send(&alice, Method::POST, &format!("/users/{b_id}/schedules/{sid}/suspend"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mine: Value = bob(Method::GET, "/v1/schedules").send().await.unwrap().json().await.unwrap();
    assert!(mine["schedules"][0]["paused"].to_string().contains("an admin"), "Bob's own list does not say an admin suspended it: {mine}");
    let (st, v) = s.send(&alice, Method::POST, &format!("/users/{b_id}/schedules/{sid}/suspend"), None).await;
    assert_eq!(st, StatusCode::CONFLICT, "suspended twice: {v}");
    // An admin's suspension is lifted only by an admin: Bob can neither
    // resume it nor pause over it himself (#262).
    let r = bob(Method::POST, &format!("/v1/schedules/{sid}/resume")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 409, "Bob lifted the admin's suspension");
    let r = bob(Method::POST, &format!("/v1/schedules/{sid}/pause")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 409, "Bob paused over the admin's suspension: {}", r.text().await.unwrap());
    let (st, v) = s.send(&alice, Method::POST, &format!("/users/{b_id}/schedules/{sid}/resume"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mine: Value = bob(Method::GET, "/v1/schedules").send().await.unwrap().json().await.unwrap();
    assert!(mine["schedules"][0]["paused"].is_null(), "resumed, it is still paused: {mine}");

    // Data: Bob's files, and one file's words.
    let (st, v) = s.send(&alice, Method::GET, &format!("/users/{b_id}/data/files"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let file = v["files"].as_array().unwrap().iter().find(|f| f["name"] == "contracts.txt").cloned().unwrap_or_else(|| panic!("{v}"));
    let fid = file["id"].as_str().unwrap().to_string();
    let (st, v) = s.send(&alice, Method::GET, &format!("/users/{b_id}/data/files/{fid}"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["files"]["text"], WORDS, "{v}");
    for kind in ["tasks", "chats", "memory"] {
        let (st, v) = s.send(&alice, Method::GET, &format!("/users/{b_id}/data/{kind}"), None).await;
        assert_eq!(st, StatusCode::OK, "{kind}: {v}");
    }
    let (st, _) = s.send(&alice, Method::GET, &format!("/users/{b_id}/data/passwords"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Each read is in the audit log: the admin, the person, what, and when.
    let (_, audit) = s.send(&alice, Method::GET, "/audit?action=user_data_read", None).await;
    let reads = audit["entries"].as_array().unwrap();
    let one = reads.iter().find(|e| e["after"]["item"] == fid.as_str()).unwrap_or_else(|| panic!("the file's read is not in the audit: {audit}"));
    // The name on the record is the appliance's own for the admin (the
    // fixture person is "alice").
    assert_eq!((one["target"].as_str(), one["after"]["resource"].as_str(), one["name"].as_str()), (Some(b_id.as_str()), Some("files"), Some("alice")), "{one}");
    assert!(one["admin"].is_string() && one["at"].is_string(), "{one}");
    assert!(reads.len() >= 5, "a read is missing from the audit: {audit}");
    // And Bob is told.
    let inbox: Value = bob(Method::GET, "/v1/inbox").send().await.unwrap().json().await.unwrap();
    assert!(inbox.to_string().contains("opened one of your files"), "Bob is not told his file was opened: {inbox}");

    // While the organisation keeps people's data private, root reads none of it.
    assert_eq!(app.api("alice").put("/v1/data-access", json!({ "private": true })).await.0.as_u16(), 204);
    let (st, v) = s.send(&alice, Method::GET, &format!("/users/{b_id}/data/files"), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(app.api("alice").put("/v1/data-access", json!({ "private": false })).await.0.as_u16(), 204);

    // Delete: never without a fresh password, and their id typed — Bob
    // is a config person with no email address.
    tokio::time::sleep(std::time::Duration::from_secs(lapse as u64 + 1)).await;
    let (st, v) = s.send(&alice, Method::DELETE, &format!("/users/{b_id}"), Some(json!({ "email": "bob" }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{v}");
    let fresh = s.session().await;
    let (st, v) = s.send(&fresh, Method::DELETE, &format!("/users/{b_id}"), Some(json!({ "email": "someone" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, v) = s.send(&fresh, Method::DELETE, &format!("/users/{b_id}"), Some(json!({ "email": "bob" }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["account_removed"], false, "a config person has no local account to remove: {v}");
    let erased = &v["erased"];
    assert_eq!((erased["schedules_ended"].as_u64(), erased["sessions_ended"].as_u64()), (Some(1), Some(1)), "{v}");
    // His sign-in to the calendar is revoked at the app, both its tokens
    // (connectors spec, Isolation).
    assert_eq!(erased["sign_ins_revoked"].as_u64(), Some(1), "{v}");
    assert_eq!(cal.state.lock().unwrap().revoked.len(), 2, "the app did not revoke Bob's refresh and access tokens");
    assert!(erased["content_removed"].as_u64().unwrap_or(0) >= 2, "{v}");

    // Gone: his data, session and schedule; the config person himself
    // stays listed, turned off, with nothing left to sign in to.
    let r = reqwest::Client::new().post(format!("{}/v1/sign-in", app.base)).json(&json!({ "person": "bob", "password": PASSWORD })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "here still signs in deleted Bob");
    let (_, users) = s.send(&fresh, Method::GET, "/users", None).await;
    let row = users["users"].as_array().unwrap().iter().find(|u| u["id"] == "bob").cloned().unwrap_or_else(|| panic!("{users}"));
    assert_eq!(row["status"], "disabled", "{row}");
    assert_eq!(bob(Method::GET, "/v1/schedules").send().await.unwrap().status().as_u16(), 401, "Bob's session still works");
    let db = app.db.admin().await;
    let left: i64 = db.query_one("SELECT count(*) FROM public.content WHERE person = 'bob'", &[]).await.unwrap().get(0);
    assert_eq!(left, 0, "Bob's content is still in the store");
    let said: i64 = db
        .query_one("SELECT count(*) FROM public.content WHERE body::text LIKE $1", &[&format!("%{}%", base64::engine::general_purpose::STANDARD.encode(WORDS))])
        .await
        .unwrap()
        .get(0);
    assert_eq!(said, 0, "Bob's file is still in the store");
    let (_, audit) = s.send(&fresh, Method::GET, "/audit?action=user_deleted", None).await;
    assert_eq!(audit["entries"][0]["target"], b_id.as_str(), "{audit}");

    // The screens, in every look and at phone width.
    sign_in_and_visit(&url, s.c.addr, "Alice", PASSWORD, "admin", false, Some("#/users")).await.unwrap_or_else(|e| panic!("{e}"));
}
