//! Security › Sign-in and Agent policy in the admin console (admin console
//! spec; invariant "Every step-up endpoint returns step_up_required without
//! a fresh check, and succeeds with one", for these), against the local
//! store and a real appliance.
//!
//! Sign-in: the rules as the kernel holds them; changed only with a fresh
//! password; a code made required means a password alone no longer signs
//! anyone in, and made optional again, it does; session lifetimes land on
//! new sessions; the lockout extends while failures continue, caps at the
//! longest wait, and a permanent one holds until an admin turns them back
//! on; the console's own lifetimes are kept.
//!
//! Agent policy: the organisation's default is shown; a problem is placed
//! by line; a draft changes nothing in force, and the test panel tries a
//! request against it and against what is live; Apply only with a fresh
//! password and only a policy that fits the schema; the app's own policy
//! then holds it; a rollback puts an earlier version back as a new one.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const STRICT: &str =
    "permit(principal, action == Action::\"execute\", resource)\n  when { resource.risk == \"read\" };\npermit(principal, action == Action::\"request_approval\", resource);\n";

struct Setup {
    app: Appliance,
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

    /// Someone's password at the app's own sign-in, with a code if given.
    async fn app_sign_in(&self, person: &str, password: &str, code: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({ "person": person, "password": password, "client": "app" });
        if let Some(c) = code {
            body["code"] = json!(c);
        }
        let r = reqwest::Client::new().post(format!("{}/v1/sign-in", self.app.base)).header("x-magnus-app", "0.5.0").json(&body).send().await.unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }
}

fn enroll(app: &Appliance, person: &str) -> String {
    let out = std::process::Command::new(magnus_invariants::appliance::bin("magnus-appliance"))
        .args(["totp-enroll", "--person", person, "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "magnus totp-enroll: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn code(uri: &str) -> String {
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).unwrap();
    let raw = magnus::totp::base32_decode(secret).unwrap();
    magnus::totp::generate(&raw, (chrono::Utc::now().timestamp().div_euclid(30)) as u64, 6)
}

/// An RFC 3339 answer as a UTC time.
fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().to_utc()
}

#[tokio::test(flavor = "multi_thread")]
async fn root_sets_the_sign_in_rules_and_edits_tests_applies_and_rolls_back_the_agent_policy() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    local_user(&app, "Carol", PASSWORD, false);
    let mine = format!("{}/32", lan_ip());
    // Long enough for a browser's sign-in to hand back its session (that
    // has taken 23 seconds after the password on a loaded machine).
    let lapse = 60;
    let p = prepare(&app, &[&mine], &[("step_up_seconds", lapse.into())]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let s = Setup { app, url: url.clone(), c, b };
    let alice = s.session().await;

    // The rules as they are: a code optional, and the console's own lifetimes.
    let (st, v) = s.send(&alice, Method::GET, "/security/sign-in", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!((v["members"]["code"].as_str(), v["admins"]["fixed"].as_bool()), (Some("optional"), Some(true)), "{v}");
    assert!(v["sessions"]["console"]["idle_minutes"].is_number() && v["lockout"]["failures"].is_number(), "{v}");
    let (st, _) = s.app_sign_in("carol", PASSWORD, None).await;
    assert_eq!(st, StatusCode::OK);

    // The agent policy: the organisation's default, shown as such.
    let (st, v) = s.send(&alice, Method::GET, "/policy/sets", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let org = v["sets"].as_array().unwrap().iter().find(|x| x["id"] == "org").cloned().unwrap();
    assert_eq!(org["default"], true, "{org}");
    assert!(org["live"].as_str().unwrap().contains("request_approval"), "{org}");
    assert!(v["people"].as_array().unwrap().iter().any(|p| p["id"] == "alice"), "{v}");

    // A problem, placed by its line and column.
    let (_, v) = s
        .send(&alice, Method::POST, "/policy/validate", Some(json!({ "cedar": format!("{STRICT}forbid(principal, action, resource) when {{ resource.colour == \"red\" }};") })))
        .await;
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["problems"][0]["line"], 4, "{v}");

    // A draft changes nothing in force; the test panel tries both.
    let (st, v) = s.send(&alice, Method::PUT, "/policy/sets/org/draft", Some(json!({ "cedar": STRICT }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let ask = json!({ "principal": "alice", "resource": "crm__update_account", "risk": "reversible_write" });
    let (st, live) = s.send(&alice, Method::POST, "/policy/test", Some(ask.clone())).await;
    assert_eq!(st, StatusCode::OK, "{live}");
    assert_eq!(live["decision"], "allow", "the default lets a reversible write go: {live}");
    let mut with = ask.clone();
    with["drafts"] = json!(true);
    let (_, drafted) = s.send(&alice, Method::POST, "/policy/test", Some(with)).await;
    assert_eq!(drafted["decision"], "ask", "the draft asks before a reversible write: {drafted}");
    assert!(drafted["rule"].as_str().is_some_and(|r| r.starts_with("org:")), "{drafted}");
    let (_, pol) = s.app.api("alice").get("/v1/policies").await;
    assert!(pol["layers"]["org"].is_null(), "a draft is in force: {pol}");
    let (_, v) = s
        .send(&alice, Method::PUT, "/policy/sets/group:sales/draft", Some(json!({ "cedar": "forbid(principal, action, resource) when { resource.risk == \"financial\" };" })))
        .await;
    assert!(v["problems"].as_array().is_some_and(|p| p.is_empty()), "{v}");

    // Once the sign-in's own step-up has lapsed, neither the sign-in rules
    // nor the policy changes without a fresh password.
    tokio::time::sleep(std::time::Duration::from_secs(lapse as u64 + 1)).await;
    let rules = json!({
        "members": { "code": "required" },
        "sessions": { "members": { "idle_minutes": 45, "max_hours": 8 }, "console": { "idle_minutes": 20, "session_hours": 6 } },
        "lockout": { "enabled": true, "failures": 3, "wait_minutes": 2, "max_wait_minutes": 10, "permanent": false },
    });
    for (m, path, body) in [
        (Method::PATCH, "/security/sign-in", rules.clone()),
        (Method::POST, "/policy/sets/org/apply", json!({})),
        (Method::POST, "/policy/sets/org/rollback", json!({ "version": 1 })),
    ] {
        let (st, v) = s.send(&alice, m, path, Some(body)).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{path}: {v}");
    }
    let (_, v) = s.send(&alice, Method::GET, "/security/sign-in", None).await;
    assert_eq!(v["members"]["code"], "optional", "the rules changed without a fresh password: {v}");

    // With one: the rules land, and a code is now required.
    let fresh = s.session().await;
    let (st, v) = s.send(&fresh, Method::PATCH, "/security/sign-in", Some(rules)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["members"]["code"], "required", "{v}");
    assert_eq!(v["sessions"]["console"], json!({ "idle_minutes": 20, "session_hours": 6 }), "{v}");
    assert_eq!(v["sessions"]["members"], json!({ "idle_minutes": 45, "max_hours": 8 }), "{v}");
    assert_eq!((v["lockout"]["failures"].as_i64(), v["lockout"]["wait_minutes"].as_i64(), v["lockout"]["max_wait_minutes"].as_i64()), (Some(3), Some(2), Some(10)), "{v}");
    let (st, _) = s.app_sign_in("carol", PASSWORD, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "a password alone still signs in with a code required");
    // An enrolled code still signs in.
    let uri = enroll(&s.app, "carol");
    let (st, v) = s.app_sign_in("carol", PASSWORD, None).await;
    assert_eq!((st, v["code_needed"].as_bool()), (StatusCode::UNAUTHORIZED, Some(true)), "{v}");
    let (st, v) = s.app_sign_in("carol", PASSWORD, Some(&code(&uri))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!((at(v["session"]["expires"].as_str().unwrap()) - chrono::Utc::now()).num_hours() >= 7, "the session is not the rules' longest: {v}");
    // Bad rules change nothing.
    let (st, _) = s.send(&fresh, Method::PATCH, "/security/sign-in", Some(json!({ "lockout": { "wait_minutes": 30, "max_wait_minutes": 5 } }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = s.send(&fresh, Method::PATCH, "/security/sign-in", Some(json!({ "members": { "code": "optional" } }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Carol's code stays enrolled: with a code optional, password alone
    // still asks whoever enrolled. A fresh unenrolled person proves the
    // rule instead.
    local_user(&s.app, "Dave", PASSWORD, false);
    let (st, _) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::OK, "with a code optional again, the password does not sign in");

    // The lockout: it extends while failures continue, caps at the
    // longest wait, and a permanent one holds until an admin's enable.
    let (st, _) = s
        .send(
            &fresh,
            Method::PATCH,
            "/security/sign-in",
            Some(json!({ "lockout": { "enabled": true, "failures": 3, "wait_minutes": 1, "max_wait_minutes": 2, "permanent": false } })),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    for _ in 0..3 {
        let (st, _) = s.app_sign_in("dave", "wrong-password", None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }
    // Locked — and the attempt itself extends the lock past the first
    // wait, since hammering a lock holds it.
    let (st, v) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "three failures did not lock: {v}");
    let first = at(v["retry_at"].as_str().unwrap());
    assert!((first - chrono::Utc::now()).num_seconds() > 90, "failing while locked did not extend past the first wait: {first}");
    // Capped: further failures hold it at the longest wait, no further.
    for _ in 0..3 {
        let (st, _) = s.app_sign_in("dave", "wrong-password", None).await;
        assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
    }
    let (st, v) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
    let longer = at(v["retry_at"].as_str().unwrap());
    assert_eq!(longer, first, "failing while locked moved the capped lock: {first} to {longer}");
    assert!((longer - chrono::Utc::now()).num_minutes() <= 2, "the lock passed the longest wait: {longer}");
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    let (st, _) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "the extended lock lapsed with the first wait");
    tokio::time::sleep(std::time::Duration::from_secs(65)).await;
    let (st, v) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::OK, "the capped lock did not lapse: {v}");
    // Permanent: held past the wait, then an enable clears it. A fresh
    // session first: the waits outlasted the last step-up.
    let fresh = s.session().await;
    let (st, _) = s.send(&fresh, Method::PATCH, "/security/sign-in", Some(json!({ "lockout": { "permanent": true } }))).await;
    assert_eq!(st, StatusCode::OK);
    for _ in 0..3 {
        s.app_sign_in("dave", "wrong-password", None).await;
    }
    let (st, v) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert!((at(v["retry_at"].as_str().unwrap()) - chrono::Utc::now()).num_days() > 365, "the permanent lock ends: {v}");
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    let (st, _) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "the permanent lock lapsed");
    let (st, v) = s.send(&fresh, Method::POST, "/users/dave/enable", None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
    let (st, v) = s.app_sign_in("dave", PASSWORD, None).await;
    assert_eq!(st, StatusCode::OK, "enabling did not clear the lock: {v}");

    // Apply: never a policy that does not fit the schema; the draft does.
    // A fresh session again: the permanent wait outlasted the step-up.
    let fresh = s.session().await;
    let (st, v) =
        s.send(&fresh, Method::POST, "/policy/sets/org/apply", Some(json!({ "cedar": "permit(principal, action, resource) when { resource.colour == \"red\" };" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(!v["problems"].as_array().unwrap().is_empty(), "{v}");
    let (st, v) = s.send(&fresh, Method::POST, "/policy/sets/org/apply", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, pol) = s.app.api("alice").get("/v1/policies").await;
    assert_eq!(pol["layers"]["org"], STRICT, "the applied policy is not in force: {pol}");
    let (_, v) = s.send(&fresh, Method::GET, "/policy/sets", None).await;
    let org = v["sets"].as_array().unwrap().iter().find(|x| x["id"] == "org").cloned().unwrap();
    assert!(org["draft"].is_null() && org["default"] == false && org["versions"] == 1, "{org}");
    let (_, live) = s.send(&fresh, Method::POST, "/policy/test", Some(ask.clone())).await;
    assert_eq!(live["decision"], "ask", "{live}");

    // A second version, then back to the first, as a third.
    let loose = format!("{STRICT}permit(principal, action == Action::\"execute\", resource) when {{ resource.risk == \"reversible_write\" }};\n");
    let (st, _) = s.send(&fresh, Method::POST, "/policy/sets/org/apply", Some(json!({ "cedar": loose }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = s.send(&fresh, Method::POST, "/policy/sets/org/rollback", Some(json!({ "version": 1 }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, h) = s.send(&fresh, Method::GET, "/policy/sets/org/history", None).await;
    let versions = h["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 3, "{h}");
    assert_eq!(versions[0]["cedar"], STRICT, "the rollback did not put version 1 back: {h}");
    let (_, pol) = s.app.api("alice").get("/v1/policies").await;
    assert_eq!(pol["layers"]["org"], STRICT);

    // A draft dropped, as a draft kept, is a console action.
    let (st, _) = s.send(&fresh, Method::DELETE, "/policy/sets/group:sales/draft", None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    // Each change is in the audit log.
    let (_, audit) = s.send(&fresh, Method::GET, "/audit", None).await;
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["sign_in_rules_changed", "policy_applied", "policy_rolled_back"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }
    let entry = |action: &str, target: &str| {
        audit["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["action"] == action && e["target"] == target)
            .cloned()
            .unwrap_or_else(|| panic!("{action} on {target} is not in the audit: {actions:?}"))
    };
    assert_eq!(entry("policy_drafted", "org")["after"]["cedar"], STRICT);
    assert!(entry("policy_draft_dropped", "group:sales")["before"]["cedar"].as_str().is_some_and(|c| c.contains("financial")));

    // The screens, in every look and at phone width.
    sign_in_and_visit(&url, s.c.addr, "Alice", PASSWORD, "admin", false, Some("#/security/policy/org")).await.unwrap_or_else(|e| panic!("{e}"));
    sign_in_and_visit(&url, s.c.addr, "Alice", PASSWORD, "admin", false, Some("#/security/sign-in")).await.unwrap_or_else(|e| panic!("{e}"));
}
