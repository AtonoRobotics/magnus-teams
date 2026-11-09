//! Settings through the HTTP API: the devices signed in as a person, the
//! owner's view of people and their roles, each person's own notification
//! and privacy choices, and requests to delete a person's data.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::io::Write;

const PASSWORD: &str = "correct horse battery";

fn set_password(app: &Appliance, person: &str) {
    let mut c = std::process::Command::new(bin("magnus-appliance"))
        .args(["password", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--person", person])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    writeln!(c.stdin.take().unwrap(), "{PASSWORD}").unwrap();
    assert!(c.wait().unwrap().success());
}

/// Sign in from an app; an API client acting with that session.
async fn signed_in(app: &Appliance, person: &str) -> Api {
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": person, "password": PASSWORD, "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    let mut a = app.api(person);
    a.token = body["token"].as_str().unwrap().to_string();
    a
}

async fn start() -> (FakeModel, Appliance) {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    (m, app)
}

/// Each sign-in is one device; another device can be signed out from here,
/// and its session ends at once. A password sign-in used to open a second,
/// unused session as well.
#[tokio::test(flavor = "multi_thread")]
async fn devices_are_listed_and_one_can_be_signed_out() {
    let (_m, app) = start().await;
    set_password(&app, "carol");
    let laptop = signed_in(&app, "carol").await;
    let phone = signed_in(&app, "carol").await;
    let (_, v) = laptop.get("/v1/sessions").await;
    let list = v["sessions"].as_array().unwrap();
    assert_eq!(list.len(), 2, "one session per sign-in: {v}");
    assert!(list.iter().all(|s| s["client"] == "app"), "{v}");
    assert_eq!(list.iter().filter(|s| s["current"] == true).count(), 1, "{v}");
    let other = list.iter().find(|s| s["current"] == false).unwrap()["id"].as_str().unwrap().to_string();

    assert_eq!(app.api("bob").send(reqwest::Method::DELETE, &format!("/v1/sessions/{other}"), None).await.0, 404, "someone else signed carol's device out");
    assert_eq!(laptop.send(reqwest::Method::DELETE, &format!("/v1/sessions/{other}"), None).await.0, 204);
    assert_eq!(phone.get("/v1/me").await.0, 401, "the signed-out device still works");
    assert_eq!(laptop.get("/v1/me").await.0, 200);
    assert_eq!(laptop.get("/v1/sessions").await.1["sessions"].as_array().unwrap().len(), 1);
}

/// The owner sees everyone's roles and where they come from, and can set
/// them; nobody else can, and the last owner cannot be removed.
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_sets_roles_and_someone_stays_owner() {
    let (_m, app) = start().await;
    let alice = app.api("alice");
    assert_eq!(app.api("carol").get("/v1/people").await.0, 403);
    let (_, v) = alice.get("/v1/people").await;
    let carol = v["people"].as_array().unwrap().iter().find(|p| p["id"] == "carol").cloned().unwrap();
    assert_eq!((carol["roles"].clone(), carol["roles_from"].clone()), (json!([]), json!("config")), "{v}");

    assert_eq!(app.api("carol").send(reqwest::Method::PUT, "/v1/people/carol/roles", Some(json!({ "roles": ["owner"] }))).await.0, 403, "carol made herself owner");
    assert_eq!(alice.send(reqwest::Method::PUT, "/v1/people/carol/roles", Some(json!({ "roles": ["developer"] }))).await.0, 204);
    assert_eq!(app.api("carol").get("/v1/me").await.1["roles"], json!(["developer"]));
    assert_eq!(app.api("carol").get("/v1/dev/runs").await.0, 200, "the role set here does not take effect");
    let (st, v) = alice.send(reqwest::Method::PUT, "/v1/people/alice/roles", Some(json!({ "roles": [] }))).await;
    assert_eq!(st, 409, "the last owner gave up the role: {v}");
    let (_, v) = alice.get("/v1/people").await;
    assert_eq!(v["people"].as_array().unwrap().iter().find(|p| p["id"] == "carol").unwrap()["roles_from"], "settings");
}

/// Two owners who take the role from each other at the same moment leave one
/// owner, never none (spec review, 2026-11-01: the check that someone stays
/// owner ran outside any lock).
#[tokio::test(flavor = "multi_thread")]
async fn two_owners_demoting_each_other_at_once_leave_one() {
    let (_m, app) = start().await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    async fn owner(a: &Api, who: &str) -> bool {
        a.get("/v1/me").await;
        let (_, v) = a.get("/v1/people").await;
        v["people"].as_array().map_or(false, |l| l.iter().any(|p| p["id"] == who && p["roles"].as_array().unwrap().iter().any(|r| r == "owner")))
    }
    for round in 0..30 {
        // Whoever is owner makes the other owner too.
        let (by, other) = if owner(&alice, "alice").await { (&alice, "carol") } else { (&carol, "alice") };
        assert_eq!(by.send(reqwest::Method::PUT, &format!("/v1/people/{other}/roles"), Some(json!({ "roles": ["owner"] }))).await.0, 204, "round {round}");
        let (a, c) = futures::join!(
            alice.send(reqwest::Method::PUT, "/v1/people/carol/roles", Some(json!({ "roles": [] }))),
            carol.send(reqwest::Method::PUT, "/v1/people/alice/roles", Some(json!({ "roles": [] }))),
        );
        let viewer = if owner(&alice, "alice").await { &alice } else { &carol };
        let left = owner(viewer, "alice").await as u8 + owner(viewer, "carol").await as u8;
        assert!(left >= 1, "round {round}: both gave the owner role away ({} and {})", a.0, c.0);
    }
}

/// Each person's notification and privacy choices start from defaults, are
/// theirs alone, and only known ones with yes or no are kept.
#[tokio::test(flavor = "multi_thread")]
async fn preferences_are_each_persons_own() {
    let (_m, app) = start().await;
    let carol = app.api("carol");
    let (_, p) = carol.get("/v1/preferences").await;
    assert_eq!(
        p,
        json!({ "notify_blocking": true, "notify_batched": false, "notify_agent_changes": true, "feedback_consent": false, "timezone": "UTC", "working_hours": { "start": "09:00", "end": "17:00" }, "quiet_hours": { "start": "22:00", "end": "07:00" }, "review_every_minutes": 60, "interruptions_per_day": 3 })
    );
    let (st, p) = carol.send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "notify_batched": true, "feedback_consent": true }))).await;
    assert!(st.is_success(), "{p}");
    assert_eq!((p["notify_batched"].clone(), p["feedback_consent"].clone(), p["notify_blocking"].clone()), (json!(true), json!(true), json!(true)));
    assert_eq!(app.api("bob").get("/v1/preferences").await.1["feedback_consent"], false, "carol's choice reached bob");
    assert_eq!(carol.send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "theme": true }))).await.0, 400);
    assert_eq!(carol.send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "notify_batched": "yes" }))).await.0, 400);
}

/// The proactive loop's settings are changeable per person, and only in
/// shapes the loop can use (#237).
#[tokio::test(flavor = "multi_thread")]
async fn proactive_settings_are_typed() {
    let (_m, app) = start().await;
    let carol = app.api("carol");
    let (st, p) = carol
        .send(
            reqwest::Method::PUT,
            "/v1/preferences",
            Some(json!({ "timezone": "America/Los_Angeles", "working_hours": { "start": "08:00", "end": "16:00" }, "interruptions_per_day": 1 })),
        )
        .await;
    assert!(st.is_success(), "{p}");
    assert_eq!(
        (p["timezone"].clone(), p["working_hours"].clone(), p["interruptions_per_day"].clone()),
        (json!("America/Los_Angeles"), json!({ "start": "08:00", "end": "16:00" }), json!(1))
    );
    for bad in [json!({ "timezone": "Mars/Olympus" }), json!({ "working_hours": { "start": "xx", "end": "17:00" } }), json!({ "interruptions_per_day": 0 })] {
        assert_eq!(carol.send(reqwest::Method::PUT, "/v1/preferences", Some(bad.clone())).await.0.as_u16(), 400, "{bad}");
    }
}

/// A request to delete a person's data goes to the owner's inbox with the
/// date it is due; only the owner decides it, and the person sees the outcome.
#[tokio::test(flavor = "multi_thread")]
async fn a_deletion_request_reaches_the_owner_and_its_outcome_the_person() {
    let (_m, app) = start().await;
    let carol = app.api("carol");
    let (st, v) = carol.post("/v1/deletion-requests", json!({ "what": "my chats" })).await;
    assert_eq!(st, 201, "{v}");
    let due = chrono::DateTime::parse_from_rfc3339(v["due"].as_str().unwrap()).unwrap();
    assert!((due.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_days() >= 29, "{v}");
    let id = v["id"].as_str().unwrap();

    let item = app.api("alice").inbox().await.into_iter().find(|i| i["id"] == id).expect("the owner was not asked");
    assert_eq!(item["kind"], "deletion_request");
    assert_eq!(carol.respond(id, json!({ "decision": "done" })).await, 403, "the person decided their own request");
    assert_eq!(app.api("bob").respond(id, json!({ "decision": "done" })).await, 404, "someone else saw the request");
    assert!(app.api("alice").respond(id, json!({ "decision": "done", "text": "Your chats are deleted." })).await.is_success());

    let (_, v) = carol.get("/v1/deletion-requests").await;
    let r = &v["requests"][0];
    assert_eq!((r["what"].clone(), r["decision"].clone(), r["note"].clone()), (json!("my chats"), json!("done"), json!("Your chats are deleted.")), "{v}");
}
