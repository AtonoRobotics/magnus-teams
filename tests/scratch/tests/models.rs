//! The model list people choose from. An adapter signed in with OAuth may
//! refresh its token while listing models: the new credential goes to the
//! vault, the list still comes back, and no caller sees the token.

use axum::{
    extract::State,
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const FRESH: &str = "fresh-access-token";

/// A provider that lists two models to the refreshed token only, and a
/// token endpoint that counts refreshes.
async fn oauth_provider() -> (String, Arc<AtomicUsize>) {
    let refreshes = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/token",
            post(|State(n): State<Arc<AtomicUsize>>| async move {
                n.fetch_add(1, Ordering::SeqCst);
                Json(json!({ "access_token": FRESH, "refresh_token": "refresh-2", "expires_in": 3600 }))
            }),
        )
        .route(
            "/models",
            get(|h: HeaderMap| async move {
                if h.get("authorization").and_then(|v| v.to_str().ok()) != Some(&format!("Bearer {FRESH}")) {
                    return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({ "error": "stale token" })));
                }
                (axum::http::StatusCode::OK, Json(json!({ "data": [{ "id": "oa-1" }, { "id": "oa-2" }] })))
            }),
        )
        .with_state(refreshes.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, refreshes)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_refreshed_while_listing_models_goes_to_the_vault_not_the_caller() {
    let (provider, refreshes) = oauth_provider().await;
    let m = FakeModel::start(script(|_| async { Reply::Text("ok".into()) })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let stale = json!({ "access_token": "stale-access-token", "refresh_token": "refresh-1", "expires_at": 0 }).to_string();
    assert!(alice.put_secret("oa_credential", &stale).await.is_success());
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let oa = json!({
        "id": "oa",
        "command": [bin("magnus-adapter-openai")],
        "settings": { "api": "chat", "base_url": provider, "credential": "oa_credential",
            "auth": { "kind": "oauth_device", "token_url": format!("{provider}/token"), "client_id": "magnus-test" } }
    });
    cfg["adapters"].as_array_mut().unwrap().push(toml::Value::try_from(oa).unwrap());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;

    let (st, v) = app.api("carol").get("/v1/models").await;
    assert!(st.is_success(), "{v}");
    let listed: Vec<Value> = v["adapters"].as_array().unwrap().iter().filter(|a| a["adapter"] == "oa").map(|a| a["models"].clone()).collect();
    assert_eq!(listed, [json!(["oa-1", "oa-2"])], "the refreshing adapter's models are missing: {v}");
    let (st, v) = alice.get("/v1/adapters/oa/catalog").await;
    assert!(st.is_success(), "{v}");
    assert_eq!(v["event"], "catalog", "{v}");
    assert!(!v.to_string().contains(FRESH) && !v.to_string().contains("refresh-2"), "the catalog handed out the credential: {v}");
    // The refreshed token was stored: the second listing did not refresh again.
    assert_eq!(refreshes.load(Ordering::SeqCst), 1, "the refreshed credential was not kept");
}

/// A rate limit is transient: the kernel waits and asks again, as for an
/// outage, rather than pausing the task (and its schedule) for a person to
/// resume. Only a provider failure that persists suspends the run.
#[tokio::test(flavor = "multi_thread")]
async fn a_rate_limit_is_retried_rather_than_pausing_the_task() {
    // The first request is refused with a 429; asked again, the model works.
    let calls = Arc::new(AtomicUsize::new(0));
    let then = steps(vec![contract_unchecked("say hello", "said"), fail("nothing to do")]);
    let model = FakeModel::start(script(move |c| {
        let (calls, then) = (calls.clone(), then.clone());
        async move {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::Status(429)
            } else {
                then(c).await
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().model_retries(2), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "failed", "a single 429 paused the task instead of being retried: {t}");
    assert!(alice.inbox().await.iter().all(|i| i["details"]["reason"] != "usage_limit"), "a person was asked to resume after one 429");
}

/// A billing refusal does not pass by itself: the task pauses on the first
/// answer and tells the person why, instead of asking again as for a rate
/// limit and reporting a usage limit (#539).
#[tokio::test(flavor = "multi_thread")]
async fn a_billing_refusal_pauses_at_once_and_says_why() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Router::new()
        .route("/models", get(|| async { Json(json!({ "data": [{ "id": "m" }] })) }))
        .route(
            "/chat/completions",
            post(|State(n): State<Arc<AtomicUsize>>| async move {
                n.fetch_add(1, Ordering::SeqCst);
                (
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": { "code": "1113", "message": "Insufficient balance or no resource package. Please recharge." } })),
                )
            }),
        )
        .with_state(calls.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, provider).await.unwrap() });

    let m = FakeModel::start(script(|_| async { Reply::Text("ok".into()) })).await;
    let mut app = Appliance::start(Spec::default().model_retries(3), &m).await;
    let alice = app.api("alice");
    assert!(alice.put_secret("oa_key", "test-key").await.is_success());
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let oa = json!({
        "id": "oa",
        "command": [bin("magnus-adapter-openai")],
        "settings": { "api": "chat", "base_url": url, "credential": "oa_key", "auth": { "kind": "api_key" } }
    });
    cfg["adapters"].as_array_mut().unwrap().push(toml::Value::try_from(oa).unwrap());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    let alice = app.api("alice");
    let (st, v) = alice.put("/v1/model", json!({ "adapter": "oa", "model": "m" })).await;
    assert!(st.is_success(), "{v}");

    let id = alice.submit("Say hello.").await;
    let item = alice.wait_item(&id, "notice").await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a billing refusal was asked again");
    assert_eq!(item["details"]["reason"], "invalid_request", "{item}");
    assert!(item.to_string().contains("Insufficient balance") && item.to_string().contains("billing"), "the person is not told the cause: {item}");
}
