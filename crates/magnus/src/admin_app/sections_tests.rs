//! The Admin sections' server behaviour, against a kernel booted on a fresh
//! database (the harness in `tests.rs`): sam owns the server, ana is an
//! admin by the config, lee by the directory, and mo is a member.

use super::guard::mark;
use super::tests::{server, Server};
use axum::Router;
use chrono::Utc;
use serde_json::{json, Value};

/// A session that has just confirmed its password.
async fn stepped(s: &Server, id: &str) -> String {
    let t = s.token(id).await;
    mark(&t, Utc::now());
    t
}

async fn patch(s: &Server, t: &str, path: &str, body: Value) -> (u16, Value) {
    s.call(t, reqwest::Method::PATCH, path, Some(body)).await
}

async fn delete(s: &Server, t: &str, path: &str, body: Value) -> (u16, Value) {
    s.call(t, reqwest::Method::DELETE, path, Some(body)).await
}

async fn role(s: &Server, t: &str, id: &str) -> Value {
    let (_, v) = s.get(t, "/v1/admin/people").await;
    v["people"].as_array().into_iter().flatten().find(|p| p["id"] == id).map(|p| p["role"].clone()).unwrap_or(Value::Null)
}

/// The last owner can be neither removed, turned off nor given another role,
/// and only an owner gives or takes the owner role. Once someone else owns
/// the server too, the first may step down.
#[tokio::test(flavor = "multi_thread")]
async fn the_last_owner_is_protected() {
    let s = server(|_| Router::new()).await;
    let (sam, ana) = (stepped(&s, "sam").await, stepped(&s, "ana").await);
    assert_eq!(role(&s, &ana, "sam").await, "owner");
    let (st, v) = delete(&s, &ana, "/v1/admin/people/sam", json!({ "projects": "delete" })).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("last_owner")), "the last owner was removed: {v}");
    let (st, v) = s.post(&ana, "/v1/admin/people/sam/disable", json!({})).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("last_owner")), "the last owner was turned off: {v}");
    let (st, v) = patch(&s, &sam, "/v1/admin/people/sam", json!({ "role": "admin" })).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("last_owner")), "the last owner stepped down: {v}");
    let (st, v) = patch(&s, &ana, "/v1/admin/people/mo", json!({ "role": "owner" })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "an admin made an owner: {v}");
    assert_eq!(patch(&s, &sam, "/v1/admin/people/mo", json!({ "role": "owner" })).await.0, 204);
    assert_eq!(role(&s, &sam, "mo").await, "owner");
    assert_eq!(patch(&s, &sam, "/v1/admin/people/sam", json!({ "role": "admin" })).await.0, 204, "an owner could not step down beside another");
    assert_eq!(role(&s, &sam, "sam").await, "admin");
    assert!(!s.audited("role_changed").is_empty(), "a role change is not in the audit");
    s.done().await;
}

/// Each person has one of the four roles. A guest invited by email is a
/// guest at once; a member made a guest is held to reading and chatting; an
/// admin made a member stops being an admin to the app.
#[tokio::test(flavor = "multi_thread")]
async fn roles_are_owner_admin_member_or_guest() {
    let s = server(|_| Router::new()).await;
    let ana = stepped(&s, "ana").await;
    let (_, v) = s.get(&ana, "/v1/admin/people").await;
    assert_eq!(v["roles"], json!(["owner", "admin", "member", "guest"]));
    for (id, want) in [("sam", "owner"), ("ana", "admin"), ("lee", "admin"), ("mo", "member")] {
        assert_eq!(role(&s, &ana, id).await, want, "{id}");
    }
    assert_eq!(patch(&s, &ana, "/v1/admin/people/mo", json!({ "role": "visitor" })).await.0, 400, "a fifth role was taken");
    // lee is the directory's only admin, which it still guards.
    let (st, v) = patch(&s, &ana, "/v1/admin/people/lee", json!({ "role": "member" })).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("last_admin")), "{v}");
    assert_eq!(patch(&s, &ana, "/v1/admin/people/mo", json!({ "role": "admin" })).await.0, 204);
    assert_eq!(patch(&s, &ana, "/v1/admin/people/lee", json!({ "role": "member" })).await.0, 204);
    let lee = s.token("lee").await;
    assert_eq!(s.get(&lee, "/v1/admin/standing").await.1["admin"], false, "a demoted admin still administers");
    assert_eq!(patch(&s, &ana, "/v1/admin/people/lee", json!({ "role": "guest" })).await.0, 204);
    assert!(super::roles::is_guest(&s.k.person("lee").unwrap()), "lee is not a guest");
    assert_eq!(role(&s, &ana, "lee").await, "guest");
    let (st, v) = s.post(&ana, "/v1/admin/people", json!({ "email": "gus@example.com", "name": "Gus", "role": "guest" })).await;
    assert_eq!(st, 201, "{v}");
    assert!(v["invite_token"].is_string() && v["role"] == "guest", "{v}");
    assert!(super::roles::is_guest(&s.k.person("gus").unwrap()), "an invited guest is not a guest");
    let (st, v) = s.post(&ana, "/v1/admin/people", json!({ "email": "own@example.com", "name": "Own", "role": "owner" })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "an admin invited an owner: {v}");
    s.done().await;
}

/// An admin's read of a person's data is in the admin audit and the log,
/// and the person is told in their inbox.
#[tokio::test(flavor = "multi_thread")]
async fn an_admins_read_is_logged_and_the_person_told() {
    let s = server(|_| Router::new()).await;
    let ana = stepped(&s, "ana").await;
    let (st, v) = s.post(&ana, "/v1/admin/people/mo/data/tasks", json!({})).await;
    assert_eq!(st, 200, "{v}");
    let audit = s.audited("person_data_read");
    assert_eq!(audit.len(), 1, "the read is not in the admin audit");
    assert_eq!(audit[0]["target"], "people/mo/data/tasks");
    let told = s.k.read(|st| st.items.values().filter(|i| i.person == "mo" && i.details["reason"] == "data_read").map(|i| i.summary.clone()).collect::<Vec<_>>());
    assert_eq!(told, vec!["Ana read your tasks".to_string()], "mo was not told");
    // A read is a change on the record: it needs the step-up.
    let fresh = s.token("ana").await;
    assert_eq!(s.post(&fresh, "/v1/admin/people/mo/data/tasks", json!({})).await.0, 403);
    s.done().await;
}

/// A workspace rule caps every person's own rules: it is checked as one of
/// each person's, and Auto Review's setting for the workspace overrides the
/// config's.
#[tokio::test(flavor = "multi_thread")]
async fn workspace_rules_cap_every_persons_and_auto_review_is_set_here() {
    let s = server(|_| Router::new()).await;
    let ana = stepped(&s, "ana").await;
    let (st, v) = s.post(&ana, "/v1/admin/rules", json!({ "effect": "allow", "connector": "slack" })).await;
    assert_eq!(st, 400, "an allow with no tool or destination was taken: {v}");
    let (st, v) = s.post(&ana, "/v1/admin/rules", json!({ "effect": "block", "connector": "slack", "words": "Never post to Slack" })).await;
    assert_eq!(st, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    for person in ["mo", "sam"] {
        let rules = s.k.rules_for(person);
        let w = rules.iter().find(|r| r.id == format!("workspace:{id}")).unwrap_or_else(|| panic!("{person} is not held to the workspace rule"));
        assert_eq!(w.effect(), crate::rules::Effect::Block);
        assert!(w.live(), "a workspace rule is not live for {person}");
    }
    assert_eq!(s.get(&ana, "/v1/admin/rules").await.1["rules"].as_array().map(Vec::len), Some(1));
    assert!(s.k.auto_review_acts(), "Auto Review does not act by the shipped default");
    assert_eq!(s.call(&ana, reqwest::Method::PUT, "/v1/admin/auto-review", Some(json!({ "acts": false }))).await.0, 200);
    assert!(!s.k.auto_review_acts(), "the workspace's setting did not hold");
    assert_eq!(delete(&s, &ana, &format!("/v1/admin/rules/{id}"), json!({})).await.0, 204);
    assert!(s.k.rules_for("mo").is_empty(), "a removed workspace rule still holds");
    assert_eq!(s.audited("workspace_rule_set").len(), 1);
    assert_eq!(s.audited("auto_review_set").len(), 1);
    s.done().await;
}

/// Removing a person needs to know what happens to what they hold, ends
/// their account, and is refused for oneself.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_person_says_what_becomes_of_their_work() {
    let s = server(|_| Router::new()).await;
    let ana = stepped(&s, "ana").await;
    assert_eq!(s.post(&ana, "/v1/admin/people", json!({ "email": "zed@example.com", "name": "Zed", "role": "member" })).await.0, 201);
    assert_eq!(delete(&s, &ana, "/v1/admin/people/zed", json!({})).await.0, 400, "a removal that said nothing of their work went ahead");
    assert_eq!(delete(&s, &ana, "/v1/admin/people/zed", json!({ "projects": "transfer" })).await.0, 400, "a transfer to no one went ahead");
    assert_eq!(delete(&s, &ana, "/v1/admin/people/ana", json!({ "projects": "delete" })).await.1["error"], "self");
    let (st, v) = delete(&s, &ana, "/v1/admin/people/zed", json!({ "projects": "transfer", "to": "mo" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["projects"], "transferred");
    assert!(s.k.person("zed").is_none(), "zed's account is still here");
    assert_eq!(s.audited("user_deleted").len(), 1);
    s.done().await;
}

/// The sections' reads answer an admin: connectors with who connected them,
/// the sign-in rules, how long things are kept, developer access, the
/// overview, backups, computers and health.
#[tokio::test(flavor = "multi_thread")]
async fn the_sections_reads_answer() {
    let s = server(|_| Router::new()).await;
    let ana = s.token("ana").await;
    for path in [
        "/v1/admin/connectors",
        "/v1/admin/sign-in-policy",
        "/v1/admin/retention",
        "/v1/admin/developer-access",
        "/v1/admin/overview",
        "/v1/admin/backups",
        "/v1/admin/computers",
        "/v1/admin/health",
        "/v1/admin/audit",
        "/v1/admin/updates",
    ] {
        let (st, v) = s.get(&ana, path).await;
        assert_eq!(st, 200, "{path}: {v}");
    }
    let (_, o) = s.get(&ana, "/v1/admin/overview").await;
    assert!(o["alerts"].is_array() && o["running"].is_object() && o["computers"]["set_up"] == false, "{o}");
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/admin/overview").await.0, 403, "a member read the overview");
    let ana = stepped(&s, "ana").await;
    let (st, v) = s.get(&ana, "/v1/admin/audit/export").await;
    assert_eq!(st, 200, "{v}");
    assert!(v["csv"].as_str().is_some_and(|c| c.starts_with("seq,at,admin")), "{v}");
    assert_eq!(s.audited("audit_exported").len(), 1, "an export left no record");
    s.done().await;
}
