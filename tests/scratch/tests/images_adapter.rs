//! Pictures made through a model adapter's own sign-in (`[images] adapter`),
//! as on a server whose only OpenAI access is a ChatGPT sign-in: the
//! kernel asks the adapter, the adapter asks the provider's Images route
//! with its refreshed token, and the picture appears behind its placeholder.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{extract::State, http::HeaderMap, http::Uri, routing::post, Json, Router};
use base64::Engine;
use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FRESH: &str = "fresh-access-token";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture";

#[derive(Default)]
struct Seen {
    refreshes: usize,
    /// Path, authorization, originator header and body of each picture asked.
    asked: Vec<(String, String, String, Value)>,
}

/// A provider with a token endpoint and the Images routes, which answer
/// the refreshed token only. The picture is the PNG bytes and the prompt.
async fn provider() -> (String, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let picture = |State(s): State<Arc<Mutex<Seen>>>, uri: Uri, h: HeaderMap, Json(body): Json<Value>| async move {
        let header = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        s.lock().unwrap().asked.push((uri.path().to_string(), header("authorization"), header("originator"), body.clone()));
        if header("authorization") != format!("Bearer {FRESH}") {
            return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({ "error": { "message": "stale token" } })));
        }
        let bytes = [PNG, body["prompt"].as_str().unwrap_or_default().as_bytes()].concat();
        (axum::http::StatusCode::OK, Json(json!({ "created": 1, "data": [{ "b64_json": base64::engine::general_purpose::STANDARD.encode(bytes) }] })))
    };
    let app = Router::new()
        .route(
            "/token",
            post(|State(s): State<Arc<Mutex<Seen>>>| async move {
                s.lock().unwrap().refreshes += 1;
                Json(json!({ "access_token": FRESH, "refresh_token": "refresh-2", "expires_in": 3600 }))
            }),
        )
        .route("/images/generations", post(picture))
        .route("/images/edits", post(picture))
        .with_state(seen.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

async fn bytes(who: &Api, path: &str, token: &str) -> (reqwest::StatusCode, Vec<u8>) {
    let r = reqwest::Client::new().get(format!("{}{path}", who.base)).bearer_auth(token).send().await.unwrap();
    (r.status(), r.bytes().await.unwrap().to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_with_its_own_sign_in_makes_the_picture_behind_a_placeholder() {
    let (backend, seen) = provider().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("draw", "drawn"),
        tool("image_generate", json!({ "prompt": "a red barn" })),
        tool("image_generate", json!({ "prompt": "a blue barn", "quality": "premium" })),
        complete("done"),
    ]))
    .await;
    let adapters = json!([
        { "id": "anthropic", "command": [bin("magnus-adapter-anthropic")], "settings": { "base_url": model.url, "credential": "anthropic_api_key" } },
        { "id": "codex", "command": [bin("magnus-adapter-openai")], "settings": {
            "api": "responses", "base_url": backend, "credential": "codex_oauth",
            "auth": { "kind": "oauth_device", "token_url": format!("{backend}/token"), "client_id": "magnus-test" },
            "headers": { "originator": "codex_cli_rs" } } },
    ]);
    let mut spec =
        Spec::default().section("adapters", adapters).section("images", json!({ "adapter": "codex", "fast_model": "gpt-image-quick", "premium_model": "gpt-image-best" }));
    let stale = json!({ "access_token": "stale-access-token", "refresh_token": "refresh-1", "expires_at": 0 }).to_string();
    spec.secrets.push(("codex_oauth".into(), stale));
    let (alice_token, bob_token) = (spec.token("alice"), spec.token("bob"));
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Draw two barns.").await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");

    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    let ids: Vec<String> = results.iter().filter_map(|t| t.split("placeholder=").nth(1)).map(|rest| rest.split(['.', ' ']).take(2).collect::<Vec<_>>().join(".")).collect();
    assert_eq!(ids.len(), 2, "two placeholders were not handed out: {results:?}");
    for (id, prompt) in ids.iter().zip(["a red barn", "a blue barn"]) {
        let path = format!("/v1/ui/images/{id}");
        let got = eventually("the picture", Duration::from_secs(20), || {
            let (who, token, path) = (&alice, &alice_token, &path);
            async move {
                let (code, b) = bytes(who, path, token).await;
                (code == 200).then_some(b)
            }
        })
        .await;
        assert_eq!(got, [PNG, prompt.as_bytes()].concat(), "the picture for {prompt:?} is not the provider's");
        assert_eq!(bytes(&alice, &path, &bob_token).await.0, 404, "another person read the picture");
    }

    let seen = seen.lock().unwrap();
    // Both pictures may start before the first refresh is stored.
    assert!((1..=2).contains(&seen.refreshes), "the adapter's expired sign-in was not refreshed: {}", seen.refreshes);
    let asked: Vec<_> = seen.asked.iter().filter(|a| a.1 == format!("Bearer {FRESH}")).collect();
    assert_eq!(asked.len(), 2, "{:?}", seen.asked);
    assert!(asked.iter().all(|a| a.0 == "/images/generations" && a.2 == "codex_cli_rs"), "not the Images route with the adapter's headers: {asked:?}");
    let body = |p: &str| asked.iter().map(|a| &a.3).find(|b| b["prompt"] == p).cloned().unwrap();
    assert_eq!((body("a red barn")["model"].clone(), body("a red barn")["quality"].clone()), (json!("gpt-image-quick"), json!("medium")), "fast is not the default");
    assert_eq!((body("a blue barn")["model"].clone(), body("a blue barn")["quality"].clone()), (json!("gpt-image-best"), json!("high")), "premium was not used when asked");
}
