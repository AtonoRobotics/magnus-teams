//! Slack, brokered (connectors spec, Brokered API connectors). The catalog's
//! Slack manifest names the image built from this checkout (`brokered/slack`),
//! which runs in each person's own microVM and reaches only `slack.com`.
//! People sign in at Slack through the user-token endpoints its metadata
//! names, at the one redirect Slack registered, without PKCE (the app has
//! not opted in). A local HTTPS host stands in for Slack's API.

use magnus_invariants::*;
use magnus_scratch::brokered::*;
use magnus_scratch::stand_in::{self, StandIn};
use magnus_scratch::*;
use serde_json::{json, Value};

const HOST: &str = "slack.com";

fn manifest() -> toml::Table {
    std::fs::read_to_string(magnus_invariants::appliance::workspace_root().join("catalog/slack.toml")).unwrap().parse().unwrap()
}

fn redirect() -> String {
    manifest()["redirect_uri"].as_str().unwrap().to_string()
}

fn answer(r: &stand_in::Request, _: &[stand_in::Request]) -> (u16, String, String) {
    if r.path.starts_with("/api/chat.postMessage") {
        stand_in::ok(json!({ "ok": true, "channel": "C0123", "ts": "1700000000.000100" }))
    } else {
        (200, String::new(), json!({ "ok": false, "error": "unknown_method" }).to_string())
    }
}

async fn slack_for_alice(m: &FakeModel, slack: &StandIn) -> (Appliance, FakeOAuthApp, Image) {
    let mf = manifest();
    let image = image("magnus-slack");
    assert_eq!(mf["endpoint"].as_str(), Some(image.digest.as_str()), "catalog/slack.toml names another image than the one built from this commit");
    let hosts: Vec<String> = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    assert_eq!(hosts, [HOST]);
    let classes: Vec<(String, String)> = mf["tools"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    let tools: Vec<(&str, &str)> = classes.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let provider = FakeOAuthApp::provider_api(3600).await;
    let mut app = Appliance::start(Spec::default(), m).await;
    let (addr, root) = (slack.addr.clone(), slack.root.clone());
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 300, "slack", &hosts, &provider.base, &tools);
        let b = cfg["brokered"].as_table_mut().unwrap();
        let mut resolve = toml::Table::new();
        resolve.insert(format!("{HOST}:443"), addr.into());
        b.insert("resolve".into(), resolve.into());
        b.insert("roots".into(), toml::Value::Array(vec![root.display().to_string().into()]));
        let c = cfg["connectors"].as_array_mut().unwrap().iter_mut().find(|c| c["name"].as_str() == Some("slack")).unwrap().as_table_mut().unwrap();
        c.insert("audience".into(), mf["audience"].clone());
        c.insert("label".into(), mf["label"].clone());
        c.insert("scopes".into(), mf["scopes"].clone());
    })
    .await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors/slack/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/slack/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);
    (app, provider, image)
}

/// Posting reaches other people, so it always asks first, and nothing
/// reaches Slack until Alice approves. With `post_message` classed
/// `reversible_write` it fails.
#[tokio::test(flavor = "multi_thread")]
async fn slack_posts_as_alice_only_once_she_approves_reaching_only_slack() {
    let slack = StandIn::start(HOST, answer).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("tell general I am here", "general has it"),
            1 => tool("slack__post_message", json!({ "channel": "C0123", "text": "I am here" })),
            _ => complete("told general"),
        }
    }))
    .await;
    let (app, provider, _image) = slack_for_alice(&m, &slack).await;
    let alice = app.api("alice");
    let task = alice.submit("Tell general I am here.").await;
    let ask = alice.wait_item(&task, "approval").await;
    assert!(ask.to_string().contains("post_message"), "{ask}");
    assert!(slack.requests().is_empty(), "Slack was called before Alice approved: {:?}", slack.calls("/api"));
    assert!(alice.respond(ask["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    let review = alice.wait_item(&task, "review").await;
    assert!(alice.respond(review["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.is_success());
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    assert_eq!(slack.calls("/api"), [("POST".to_string(), "/chat.postMessage".to_string())]);
    let sent = serde_json::from_str::<Value>(&slack.requests()[0].body).unwrap();
    assert_eq!(sent, json!({ "channel": "C0123", "text": "I am here" }));
    for r in slack.requests() {
        assert_eq!((r.sni.as_str(), r.host.as_str()), (HOST, HOST), "{r:?}");
        assert!(r.path.starts_with("/api/"), "{r:?}");
        assert!(r.auth.strip_prefix("Bearer ").is_some_and(|t| provider.issued(t)), "not Alice's access token: {r:?}");
    }
    assert!(!app.logs().contains("the egress proxy refused"), "the server asked for a host other than Slack's");
}

/// Slack's sign-in is the catalog's: the user-token page its metadata
/// names, Alpha Vector's client, the manifest's access, no PKCE, and the
/// one redirect Slack registered. A different address is refused before
/// Slack is asked.
#[tokio::test(flavor = "multi_thread")]
async fn slack_signs_in_at_slacks_page_through_its_registered_address_without_pkce() {
    let path = magnus_invariants::appliance::workspace_root().join("catalog/registrations.toml");
    let r: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let id = r["slack"]["client_id"].as_str().unwrap().to_string();
    let secret = "not-used-before-the-token-exchange";
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (_, list) = carol.get("/v1/connectors").await;
    let card = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == "slack").cloned().unwrap_or_else(|| panic!("slack is not in the Library: {list}"));
    assert_eq!(
        (card["title"].as_str(), card["ready"].as_bool(), card["redirect_uri"].as_str(), card["handoff"].as_str()),
        (Some("Slack"), Some(false), Some(redirect().as_str()), Some("http://127.0.0.1:17321/callback")),
        "{card}"
    );
    assert!(!carol.post("/v1/connectors/slack/sign-in", json!({ "redirect_uri": redirect() })).await.0.is_success());
    assert_eq!(alice.put("/v1/registrations/slack", json!({ "client_secret": secret })).await.0, 204);
    let (st, v) = carol.post("/v1/connectors/slack/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(!st.is_success(), "a redirect Slack did not register was accepted: {v}");
    let (st, v) = carol.post("/v1/connectors/slack/sign-in", json!({ "redirect_uri": redirect() })).await;
    assert!(st.is_success(), "Magnus could not find Slack's sign-in: {v}");
    let u = url::Url::parse(v["authorize_url"].as_str().unwrap()).unwrap();
    assert_eq!((u.host_str(), u.path()), (Some("slack.com"), "/oauth/v2_user/authorize"), "{u}");
    let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
    assert_eq!(q.get("client_id").map(String::as_str), Some(id.as_str()), "{u}");
    assert_eq!(q.get("redirect_uri").map(String::as_str), Some(redirect().as_str()), "{u}");
    assert!(q.get("code_challenge").is_none() && q.get("code_challenge_method").is_none(), "PKCE was sent, and this Slack app has not opted in: {u}");
    let state = v["state"].as_str().unwrap();
    let code = "never-log-this-code";
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let bounced = http.get(format!("{}/oauth/slack/callback?code={code}&state={state}&iss=https://mcp.slack.com", carol.base)).send().await.unwrap();
    assert_eq!(bounced.status(), 302);
    let loc = bounced.headers().get("location").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    let body = bounced.text().await.unwrap();
    assert!(body.is_empty(), "the page echoed the code: {body}");
    assert!(loc.contains(code), "{loc}");
    let back: std::collections::HashMap<String, String> = url::Url::parse(&loc).unwrap().query_pairs().into_owned().collect();
    assert_eq!(back.get("code").map(String::as_str), Some(code));
    assert_eq!(back.get("state").map(String::as_str), Some(state));
    assert!(loc.starts_with("http://127.0.0.1:17321/callback?"), "{loc}");
    let missed = http.get(format!("{}/oauth/slack/callback?code={code}&state=not-a-sign-in", carol.base)).send().await.unwrap();
    assert_eq!(missed.status(), 404);
    let missed_body = missed.text().await.unwrap();
    assert!(!missed_body.contains(code), "a refused callback echoed the code");
    assert!(!app.logs().contains(code), "the code was written to the log");
    let scope = q.get("scope").map(String::as_str).unwrap_or("");
    for s in ["search:read", "chat:write", "im:history", "groups:history", "mpim:read", "users:read"] {
        assert!(scope.split(' ').any(|x| x == s), "{s} was not asked for: {u}");
    }
    assert!(!scope.split(' ').any(|x| x.contains("email")), "more access than the tools need: {u}");
}
