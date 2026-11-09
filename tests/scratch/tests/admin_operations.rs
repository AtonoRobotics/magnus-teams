//! Operations in the admin console (admin console spec, Operations; the
//! invariant "Every step-up endpoint returns step_up_required without a
//! fresh check, and succeeds with one", for installing, rolling back and the
//! network allowlist), against the local store and a real appliance: the
//! kill switch says what it froze; a run module turned off for new tasks;
//! the model providers listed, tested, the default's kept enabled; updates
//! installed and rolled back only with a fresh password, the desktop app's
//! channel for everyone set; the network's allowlist changed only with a
//! fresh password and never to shut out the admin's own address; Health
//! names the tunnel and the microVMs; each change audited.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

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
async fn root_runs_the_appliance_modules_providers_updates_and_network_from_operations() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    // Long enough for a browser's sign-in to hand back its session (that
    // has taken 23 seconds after the password on a loaded machine).
    let lapse = 60;
    let p = prepare(&app, &[&mine], &[("step_up_seconds", lapse.into())]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let s = Setup { url: url.clone(), c, b };
    let alice = s.session().await;

    // The kill switch says what it holds, microVMs included.
    let (st, k) = s.send(&alice, Method::GET, "/kill-switch", None).await;
    assert_eq!(st, StatusCode::OK, "{k}");
    assert!(k["microvms_frozen"].is_number() && k["microvms_running"].is_number(), "{k}");

    // Modules: each with what it provides; the verifier turned off for new
    // tasks, then back to the config's.
    let (_, v) = s.send(&alice, Method::GET, "/modules", None).await;
    let mods = v["modules"].as_array().unwrap();
    assert!(mods.iter().any(|x| x["name"] == "verifier" && x["provides"].is_string() && x["version"].is_string()), "{v}");
    let (st, r) = s.send(&alice, Method::PATCH, "/modules/verifier", Some(json!({ "on": false }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, v) = s.send(&alice, Method::GET, "/modules", None).await;
    let verifier = v["modules"].as_array().unwrap().iter().find(|x| x["name"] == "verifier").cloned().unwrap();
    assert_eq!((verifier["on"].as_bool(), verifier["set_by_admin"].as_bool()), (Some(false), Some(true)), "{verifier}");
    let (st, _) = s.send(&alice, Method::PATCH, "/modules/verifier", Some(json!({ "on": null }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = s.send(&alice, Method::PATCH, "/modules/no_such_module", Some(json!({ "on": true }))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // A module is part of the release: no package to install or remove.
    let (st, _) = s.send(&alice, Method::DELETE, "/modules/verifier", None).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Model providers: listed with their models, tested; the default's stays enabled.
    let (st, v) = s.send(&alice, Method::GET, "/providers", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let def = v["providers"].as_array().unwrap().iter().find(|p| p["default"] == true).cloned().unwrap_or_else(|| panic!("{v}"));
    let id = def["id"].as_str().unwrap().to_string();
    assert!(def["enabled"] == true && def.get("settings").is_some(), "{def}");
    let (st, t) = s.send(&alice, Method::POST, &format!("/providers/{id}/test"), None).await;
    assert_eq!(st, StatusCode::OK, "{t}");
    assert!(t["ok"].is_boolean() && t["ms"].is_number(), "{t}");
    let (st, r) = s.send(&alice, Method::PATCH, &format!("/providers/{id}"), Some(json!({ "enabled": false }))).await;
    assert_eq!(st, StatusCode::CONFLICT, "the default model's provider was disabled: {r}");

    // Updates: what runs; the channel for everyone set.
    let (st, u) = s.send(&alice, Method::GET, "/updates", None).await;
    assert_eq!(st, StatusCode::OK, "{u}");
    assert!(u["version"].is_string() && u["desktop"]["channels"].as_array().is_some_and(|c| c.len() == 3), "{u}");
    let (st, r) = s.send(&alice, Method::PATCH, "/updates/desktop-channel", Some(json!({ "channel": "test" }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(s.send(&alice, Method::GET, "/updates", None).await.1["desktop"]["everyone"], "test");
    let (st, _) = s.send(&alice, Method::PATCH, "/updates/desktop-channel", Some(json!({ "channel": "nightly" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // The network: the console's names and subnets, and no tunnel here.
    let (st, n) = s.send(&alice, Method::GET, "/network", None).await;
    assert_eq!(st, StatusCode::OK, "{n}");
    assert_eq!((n["tunnel"]["configured"].as_bool(), n["admin"]["public"].as_bool()), (Some(false), Some(false)), "{n}");
    assert_eq!(n["allow"], json!([mine]), "{n}");
    let (_, h) = s.send(&alice, Method::GET, "/health", None).await;
    assert!(h["services"].as_array().unwrap().iter().any(|x| x["name"].as_str().is_some_and(|n| n.starts_with("Tunnel"))) && h["microvms_running"].is_number(), "{h}");

    // Without a fresh password: no install, no rollback, no allowlist change.
    tokio::time::sleep(std::time::Duration::from_secs(lapse as u64 + 1)).await;
    let wider = json!({ "allow": [mine.clone(), "10.99.0.0/24"] });
    for (m, path, body) in [(Method::POST, "/updates/install", None), (Method::POST, "/updates/rollback", None), (Method::PATCH, "/network/allowlist", Some(wider.clone()))] {
        let (st, v) = s.send(&alice, m, path, body).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{path}: {v}");
    }

    // With one.
    let fresh = s.session().await;
    let (st, v) = s.send(&fresh, Method::POST, "/updates/install", None).await;
    assert!(st.is_success(), "{v}");
    assert!(v["status"].is_string(), "{v}");
    let (st, v) = s.send(&fresh, Method::POST, "/updates/rollback", None).await;
    assert_eq!(st, StatusCode::CONFLICT, "nothing is kept to go back to here: {v}");
    // A list that leaves out the admin's own address is refused, loopback too.
    let (st, v) = s.send(&fresh, Method::PATCH, "/network/allowlist", Some(json!({ "allow": ["10.99.0.0/24"] }))).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("shuts_you_out")), "{v}");
    let (st, _) = s.send(&fresh, Method::PATCH, "/network/allowlist", Some(json!({ "allow": [mine.clone(), "127.0.0.0/8"] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = s.send(&fresh, Method::PATCH, "/network/allowlist", Some(wider.clone())).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, n) = s.send(&fresh, Method::GET, "/network", None).await;
    assert_eq!(st, StatusCode::OK, "the admin lost the console after the change");
    assert_eq!(n["allow"], wider["allow"], "{n}");

    // Each change is in the audit log.
    let (_, audit) = s.send(&fresh, Method::GET, "/audit", None).await;
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["module_set", "desktop_channel_set", "update_installed", "network_allowlist_changed"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }

    // The screens, in every look and at phone width.
    for screen in ["#/operations/modules", "#/operations/providers", "#/operations/updates", "#/operations/network"] {
        sign_in_and_visit(&url, s.c.addr, "Alice", PASSWORD, "admin", false, Some(screen)).await.unwrap_or_else(|e| panic!("{e}"));
    }
}
