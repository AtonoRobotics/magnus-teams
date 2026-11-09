//! Who sees an app in their Library (connectors spec, Org policy and
//! isolation: "For groups: allowed or blocked per group, and blocked
//! connectors disappear from those users' Library"). An admin allows Gmail
//! for staff and blocks it for finance, through the kernel as the console
//! does: Carol (staff) has it; Bob (staff and finance) and Dana (neither)
//! do not, cannot open its page and cannot start signing in to it; the
//! owner still manages it, marked as not hers when it is not. Cleared, it
//! is everyone's again. The boundary decides the same (`may_use`, unit
//! tests in boundary.rs).

use magnus_invariants::appliance::Person;
use magnus_invariants::*;
use serde_json::{json, Value};

/// One request over the kernel's control socket: its status and answer.
async fn control(app: &Appliance, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(app.state_dir.join("control.sock")).await.expect("the kernel's control socket");
    let b = body.map(|b| b.to_string()).unwrap_or_default();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: kernel\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

async fn library(app: &Appliance, who: &str) -> Vec<Value> {
    let (st, v) = app.api(who).get("/v1/connectors").await;
    assert!(st.is_success(), "{who}: {v}");
    v["connectors"].as_array().unwrap().clone()
}

async fn has_gmail(app: &Appliance, who: &str) -> bool {
    library(app, who).await.iter().any(|c| c["name"] == "google-gmail")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_blocked_for_a_group_leaves_that_groups_library_and_cannot_be_signed_in_to() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut spec = Spec::default();
    spec.people.push(Person {
        id: "dana".into(),
        name: "Dana".into(),
        token: format!("tok-dana-{}", uuid::Uuid::new_v4().simple()),
        groups: vec!["contractors".into()],
        roles: vec![],
    });
    let app = Appliance::start(spec, &m).await;
    for who in ["alice", "bob", "carol", "dana"] {
        assert!(has_gmail(&app, who).await, "{who} lacks Gmail before any rule");
    }

    // Allowed for staff, blocked for finance.
    let (st, v) =
        control(&app, "PATCH", "/v1/admin/connectors/google-gmail", Some(json!({ "actor": "admin:root", "access_groups": ["staff"], "blocked_groups": ["finance"] }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["access_groups"].clone(), v["blocked_groups"].clone()), (json!(["staff"]), json!(["finance"])), "{v}");
    assert!(has_gmail(&app, "carol").await, "Carol is staff, not finance: she should have Gmail");
    assert!(!has_gmail(&app, "bob").await, "Bob is in finance, which Gmail is blocked for");
    assert!(!has_gmail(&app, "dana").await, "Dana is not staff");

    // Neither Bob nor Dana opens its page or starts signing in.
    for who in ["bob", "dana"] {
        let (st, _) = app.api(who).get("/v1/connectors/google-gmail").await;
        assert_eq!(st.as_u16(), 404, "{who} opened Gmail's page");
        let (st, e) = app.api(who).post("/v1/connectors/google-gmail/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
        assert_eq!(st.as_u16(), 403, "{who} started signing in to Gmail: {e}");
    }

    // The owner still manages it, marked as hers or not by her own groups.
    let mine = library(&app, "alice").await;
    let g = mine.iter().find(|c| c["name"] == "google-gmail").expect("the owner no longer sees Gmail to manage it");
    assert_eq!(g["for_you"], true, "Alice is staff and not finance: {g}");
    assert_eq!(g["blocked_groups"], json!(["finance"]), "{g}");

    // Only the blocked groups change: who may use it stays.
    let (st, v) = control(&app, "PATCH", "/v1/admin/connectors/google-gmail", Some(json!({ "actor": "admin:root", "blocked_groups": [] }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["access_groups"], json!(["staff"]), "clearing the block changed who may use it: {v}");
    assert!(has_gmail(&app, "bob").await, "Bob is staff and no longer blocked");
    assert!(!has_gmail(&app, "dana").await, "Dana is still not staff");

    // Everyone's again.
    let (st, _) = control(&app, "PATCH", "/v1/admin/connectors/google-gmail", Some(json!({ "actor": "admin:root", "access_groups": [] }))).await;
    assert_eq!(st, 200);
    assert!(has_gmail(&app, "dana").await, "Gmail is everyone's again");
}
