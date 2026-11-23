//! A refresh on its way never undoes a Disconnect, a new sign-in or more
//! access asked for, and one sign-in's slow refresh holds up no other's.

use crate::admin_app::tests::{server_with, Server};
use crate::connectors::connection_name;
use crate::connectors::sign_in::Tokens;
use axum::Router;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const APPS: &str =
    "[[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\ncredential = \"connector_crm\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n";

/// The app's authorization server: its token endpoint answers a refresh
/// only when let go, and its revocation endpoint counts what it revokes.
struct App {
    token: String,
    revoke: String,
    arrived: Arc<Notify>,
    go: Arc<Notify>,
    revoked: Arc<AtomicUsize>,
}

async fn app() -> App {
    let (arrived, go, revoked) = (Arc::new(Notify::new()), Arc::new(Notify::new()), Arc::new(AtomicUsize::new(0)));
    let (a, g, r) = (arrived.clone(), go.clone(), revoked.clone());
    let router = Router::new()
        .route(
            "/token",
            axum::routing::post(move || {
                let (a, g) = (a.clone(), g.clone());
                async move {
                    a.notify_one();
                    g.notified().await;
                    axum::Json(json!({ "access_token": "refreshed", "refresh_token": "rt2", "expires_in": 3600 }))
                }
            }),
        )
        .route(
            "/revoke",
            axum::routing::post(move || {
                let r = r.clone();
                async move {
                    r.fetch_add(1, Ordering::SeqCst);
                    axum::http::StatusCode::OK
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router).await });
    App { token: format!("{base}/token"), revoke: format!("{base}/revoke"), arrived, go, revoked }
}

fn tokens(app: &App, access: &str, expires_at: i64) -> Value {
    json!({ "access_token": access, "refresh_token": "rt", "expires_at": expires_at, "lifetime": 3600,
            "client": { "token_endpoint": app.token, "revocation_endpoint": app.revoke, "client_id": "magnus", "resource": "r" } })
}

fn sign_in(s: &Server, person: &str, t: Value) {
    s.k.vault.put(&connection_name("crm", person), &t.to_string()).unwrap();
}

fn stored(s: &Server, person: &str) -> Option<Value> {
    s.k.vault.get(&connection_name("crm", person)).unwrap().map(|r| serde_json::from_str(r.expose()).unwrap())
}

/// mo's expired sign-in, its refresh started and waiting at the app.
async fn refreshing(s: &Server, a: &App) -> tokio::task::JoinHandle<Option<String>> {
    sign_in(s, "mo", tokens(a, "old", 0));
    let (k, c) = (s.k.clone(), s.k.connector("crm").unwrap());
    let refresh = tokio::spawn(async move { k.connector_secret(&c, "mo").await.unwrap().map(|t| t.expose().to_string()) });
    a.arrived.notified().await;
    refresh
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disconnect_during_a_refresh_stays_disconnected() {
    let s = server_with(APPS, |_| Router::new()).await;
    let a = app().await;
    let refresh = refreshing(&s, &a).await;
    assert!(s.k.disconnect("crm", "mo", "person:mo").await.unwrap().is_some());
    a.go.notify_one();
    assert_eq!(refresh.await.unwrap(), None, "a refresh gave out a token after Disconnect");
    assert!(stored(&s, "mo").is_none(), "a refresh wrote the sign-in back after Disconnect");
    assert!(!s.k.connected(&s.k.connector("crm").unwrap(), "mo"));
    // The old tokens at Disconnect, then those the refresh was given after it.
    assert_eq!(a.revoked.load(Ordering::SeqCst), 4);
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_sign_in_during_a_refresh_is_kept() {
    let s = server_with(APPS, |_| Router::new()).await;
    let a = app().await;
    let refresh = refreshing(&s, &a).await;
    let later = chrono::Utc::now().timestamp() + 3600;
    let fresh: Tokens = serde_json::from_value(tokens(&a, "new, with more access", later)).unwrap();
    s.k.keep_sign_in("crm", "mo", fresh).await.unwrap();
    a.go.notify_one();
    assert_eq!(refresh.await.unwrap().as_deref(), Some("new, with more access"));
    assert_eq!(stored(&s, "mo").unwrap()["access_token"], "new, with more access", "a refresh wrote the old sign-in over the new one");
    assert_eq!(a.revoked.load(Ordering::SeqCst), 0, "the new sign-in's grant was revoked");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn access_asked_for_during_a_refresh_is_kept() {
    let s = server_with(APPS, |_| Router::new()).await;
    let a = app().await;
    let refresh = refreshing(&s, &a).await;
    s.k.want_scopes("crm", "mo", &["write".to_string()]).unwrap();
    a.go.notify_one();
    assert_eq!(refresh.await.unwrap().as_deref(), Some("refreshed"));
    let t = stored(&s, "mo").unwrap();
    assert_eq!((t["access_token"].as_str(), t["wants"].clone()), (Some("refreshed"), json!(["write"])), "{t}");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_slow_refresh_holds_up_no_other_sign_in() {
    let s = server_with(APPS, |_| Router::new()).await;
    let a = app().await;
    let refresh = refreshing(&s, &a).await;
    sign_in(&s, "ana", tokens(&a, "ana's", chrono::Utc::now().timestamp() + 3600));
    let c = s.k.connector("crm").unwrap();
    let other = tokio::time::timeout(Duration::from_secs(5), s.k.connector_secret(&c, "ana")).await;
    let other = other.expect("one person's slow refresh held up another's app call").unwrap();
    assert_eq!(other.map(|t| t.expose().to_string()).as_deref(), Some("ana's"));
    a.go.notify_one();
    assert_eq!(refresh.await.unwrap().as_deref(), Some("refreshed"));
    s.done().await;
}
