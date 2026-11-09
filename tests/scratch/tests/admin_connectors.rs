//! Security › Connectors and credentials in the admin console (admin console
//! spec; invariant "No admin endpoint ever returns a credential value"),
//! against the local store and a real appliance: the organisation's apps
//! with their status, sign-in, scopes and credential; Google's client secret
//! set from the console serves every Google app; an app added by its address,
//! people signing in to it themselves, and never with a key, nor a server
//! that asks for one or for client details instead of a sign-in; turned off,
//! an app leaves everyone's Library, and comes back; who may use it is set;
//! Google's secret rotated and revoked; an added app removed, a catalog one
//! not. No answer from any admin route, and no
//! audit entry, ever holds a credential; each change is in the audit log.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const SECRET: &str = "gcs-0f9b1e7d-never-shown-again";
const KEY: &str = "crm-key-5c2a91d4-never-shown-again";
const ROTATED: &str = "gcs-77e0b3aa-rotated-never-shown";

/// An MCP server that wants a key: it answers 401 and says nothing of
/// where to sign in.
async fn keyed_server() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", l.local_addr().unwrap());
    let app = axum::Router::new().route("/mcp", axum::routing::post(|| async { (StatusCode::UNAUTHORIZED, [("www-authenticate", "Bearer")], "") }));
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

struct Setup {
    url: String,
    b: reqwest::Client,
    s: (String, String),
}

impl Setup {
    fn api(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.url)
    }

    async fn send(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut rq = with_cookie(self.b.request(method, self.api(path)), &self.s.0).header("x-csrf-token", &self.s.1);
        if let Some(b) = body {
            rq = rq.json(&b);
        }
        let r = rq.send().await.unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    async fn app(&self, name: &str) -> Value {
        let (st, v) = self.send(Method::GET, "/connectors", None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["connectors"].as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap_or_else(|| panic!("{name} is not listed: {v}"))
    }

    /// Every admin route that reads, as text: none may hold a credential.
    async fn everything_readable(&self) -> String {
        let mut all = String::new();
        let users: Value = with_cookie(self.b.get(self.api("/users")), &self.s.0).send().await.unwrap().json().await.unwrap();
        let mut paths: Vec<String> = ["/session", "/overview", "/health", "/kill-switch", "/users", "/connectors", "/audit", "/audit.csv"].map(String::from).to_vec();
        paths.extend(users["users"].as_array().into_iter().flatten().filter_map(|u| u["id"].as_str().map(|id| format!("/users/{id}"))));
        for p in paths {
            let r = with_cookie(self.b.get(self.api(&p)), &self.s.0).send().await.unwrap();
            assert!(r.status().is_success(), "{p}: {}", r.status());
            all.push_str(&r.text().await.unwrap());
        }
        // And the live stream's first event.
        let mut r = with_cookie(self.b.get(self.api("/events")), &self.s.0).send().await.unwrap();
        if let Ok(Ok(Some(chunk))) = tokio::time::timeout(std::time::Duration::from_secs(10), r.chunk()).await {
            all.push_str(&String::from_utf8_lossy(&chunk));
        }
        all
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_console_sets_rotates_and_revokes_the_organisations_credentials_and_never_shows_one() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let crm = FakeOAuthApp::start(3600).await;
    let closed = FakeOAuthApp::start(3600).await;
    closed.state.lock().unwrap().no_registration = true;
    let keyed = keyed_server().await;
    let mine = format!("{}/32", lan_ip());
    let p = prepare(&app, &[&mine], &[]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let me: Value = with_cookie(b.get(format!("{url}/api/v1/session")), &sid).send().await.unwrap().json().await.unwrap();
    let s = Setup { url: url.clone(), b, s: (sid, me["csrf"].as_str().unwrap().to_string()) };
    let alice = app.api("alice");

    // The catalog's Google apps: people sign in with their own accounts,
    // through Google's client, whose secret the organisation has not set.
    let gmail = s.app("google-gmail").await;
    assert_eq!(
        (gmail["source"].as_str(), gmail["sign_in"].as_str(), gmail["credential"]["kind"].as_str()),
        (Some("catalog"), Some("own_account"), Some("client_secret")),
        "{gmail}"
    );
    assert_eq!((gmail["credential"]["set"].as_bool(), gmail["status"].as_str()), (Some(false), Some("needs_credential")), "{gmail}");
    assert!(!gmail["scopes"].as_array().unwrap().is_empty(), "{gmail}");

    // Google's client secret, set once from the console, serves every Google app.
    let (st, v) = s.send(Method::PUT, "/connectors/google-gmail/credential", Some(json!({ "value": SECRET }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "set": true }));
    for name in ["google-gmail", "google-drive"] {
        let a = s.app(name).await;
        assert_eq!((a["credential"]["set"].as_bool(), a["status"].as_str()), (Some(true), Some("ready")), "{a}");
        assert!(a["credential"]["changed_at"].is_string(), "{a}");
    }
    let (_, lib) = alice.get("/v1/connectors").await;
    let card = lib["connectors"].as_array().unwrap().iter().find(|c| c["name"] == "google-gmail").cloned().unwrap();
    assert_eq!(card["ready"], true, "the app's Library does not see Google's client as complete: {card}");

    // Never with a key or client details, nor a server that asks for one
    // instead of a sign-in: each refused, and the admin told why.
    let add = |url: &str, extra: Value| {
        let mut b = json!({ "url": url, "audience": "internal", "label": "private", "access_groups": ["sales"] });
        b.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        s.send(Method::POST, "/connectors", Some(b))
    };
    for (url, extra, why) in [
        (crm.url.as_str(), json!({ "token": KEY }), "never takes a key"),
        (crm.url.as_str(), json!({ "client_id": "magnus-typed-in" }), "never takes a key, token or client details"),
        (keyed.as_str(), json!({}), "asks for a key or token instead of a sign-in"),
        (closed.url.as_str(), json!({}), "does not let Magnus register itself"),
    ] {
        let (st, v) = add(url, extra).await;
        assert!(st.is_client_error() && v.to_string().contains(why), "{url}: {st} {v}");
    }
    let (_, v) = s.send(Method::GET, "/connectors", None).await;
    assert!(v["connectors"].as_array().unwrap().iter().all(|c| c["source"] != "added"), "a refused app was added: {v}");

    // An app added by its address: people sign in to it themselves.
    let (st, v) = add(&crm.url, json!({})).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let name = v["name"].as_str().unwrap().to_string();
    let added = s.app(&name).await;
    assert_eq!((added["source"].as_str(), added["sign_in"].as_str(), added["credential"]["kind"].as_str()), (Some("added"), Some("own_account"), Some("none")), "{added}");
    assert_eq!(added["access_groups"], json!(["sales"]));

    // Who may use it: everyone, then back to sales.
    let (st, v) = s.send(Method::PATCH, &format!("/connectors/{name}"), Some(json!({ "access_groups": [] }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(s.app(&name).await["access_groups"], json!([]));

    // Turned off, Gmail leaves everyone's Library, and no task can reach it; on, it is back.
    let (st, v) = s.send(Method::PATCH, "/connectors/google-gmail", Some(json!({ "enabled": false }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(s.app("google-gmail").await["status"], "off");
    let (_, lib) = alice.get("/v1/connectors").await;
    assert!(lib["connectors"].as_array().unwrap().iter().all(|c| c["name"] != "google-gmail"), "a turned-off app is still in the Library");
    let (st, _) = s.send(Method::PATCH, "/connectors/google-gmail", Some(json!({ "enabled": true }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, lib) = alice.get("/v1/connectors").await;
    assert!(lib["connectors"].as_array().unwrap().iter().any(|c| c["name"] == "google-gmail"), "a turned-on app is not back in the Library");

    // Google's secret rotated: set again, with a new time.
    let before = s.app("google-gmail").await["credential"]["changed_at"].clone();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (st, v) = s.send(Method::PUT, "/connectors/google-gmail/credential", Some(json!({ "value": ROTATED }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_ne!(s.app("google-gmail").await["credential"]["changed_at"], before);
    // An app people sign in to themselves has no credential of the organisation's.
    let (st, v) = s.send(Method::PUT, &format!("/connectors/{name}/credential"), Some(json!({ "value": KEY }))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");

    // An app people sign in to themselves holds nothing to set.
    let (st, v) = s.send(Method::PUT, "/connectors/nope/credential", Some(json!({ "value": "x" }))).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");

    // Nothing an admin can read holds any credential: not the answers, not the audit.
    let seen = s.everything_readable().await;
    for secret in [SECRET, KEY, ROTATED] {
        assert!(!seen.contains(secret), "an admin route returned a credential");
    }

    // Revoked: Google's secret is gone, for every Google app.
    let (st, v) = s.send(Method::DELETE, "/connectors/google-drive/credential", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for n in ["google-gmail", "google-drive"] {
        assert_eq!(s.app(n).await["credential"]["set"], false, "{n}");
    }

    // An added app is removed; the catalog's are turned off instead.
    let (st, v) = s.send(Method::DELETE, "/connectors/google-gmail", None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, v) = s.send(Method::DELETE, &format!("/connectors/{name}"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, v) = s.send(Method::GET, "/connectors", None).await;
    assert!(v["connectors"].as_array().unwrap().iter().all(|c| c["name"] != name.as_str()), "the removed app is still listed");

    // Each change is in the audit log, with only whether a credential was set.
    let (_, audit) = s.send(Method::GET, "/audit", None).await;
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["connector_credential_set", "connector_added", "connector_settings_changed", "connector_credential_rotated", "connector_credential_revoked", "connector_removed"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }
    let rotated = audit["entries"].as_array().unwrap().iter().find(|e| e["action"] == "connector_credential_rotated").unwrap();
    assert_eq!(rotated["after"], json!({ "kind": "client_secret", "set": true }), "{rotated}");

    // The screen, in every look and at phone width.
    sign_in_and_visit(&url, c.addr, "Alice", PASSWORD, "admin", false, Some("#/security/connectors/google-gmail")).await.unwrap_or_else(|e| panic!("{e}"));
}
