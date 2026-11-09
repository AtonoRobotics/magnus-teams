//! The composer's model picker follows the appliance default (#540): a pick
//! made under one default does not keep sending that model after it changes.

use axum::{routing::get, Json, Router};
use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

/// A second provider, so there are models to pick besides the default. Runs
/// on it fail at the provider; this test reads only what the app sends.
async fn catalog() -> String {
    let app = Router::new().route("/models", get(|| async { Json(json!({ "data": [{ "id": "m-a" }, { "id": "m-b" }] })) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_default_replaces_an_earlier_pick_in_the_composer() {
    let provider = catalog().await;
    let m = FakeModel::start(script(|_| async { Reply::Text("ok".into()) })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    assert!(app.api("alice").put_secret("oa_key", "test-key").await.is_success());
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let oa = json!({
        "id": "oa",
        "command": [bin("magnus-adapter-openai")],
        "settings": { "api": "chat", "base_url": provider, "credential": "oa_key", "auth": { "kind": "api_key" } }
    });
    cfg["adapters"].as_array_mut().unwrap().push(toml::Value::try_from(oa).unwrap());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    assert!(set_password(&app, "alice", PASSWORD));

    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests/model-picker.mjs");
    let out = tokio::process::Command::new("node")
        .arg(script)
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}
