//! A Link refresh on its way never undoes the person ending their Link
//! sign-in.

use super::{access, revoke, token_name, Tokens};
use crate::admin_app::tests::server_with;
use crate::payments::LinkSettings;
use axum::Router;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::test(flavor = "multi_thread")]
async fn ending_link_during_a_refresh_stays_ended() {
    let s = server_with("", |_| Router::new()).await;
    let (arrived, go, revoked) = (Arc::new(Notify::new()), Arc::new(Notify::new()), Arc::new(AtomicUsize::new(0)));
    let (a, g, r) = (arrived.clone(), go.clone(), revoked.clone());
    let link = Router::new()
        .route(
            "/auth/token",
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
            "/auth/revoke",
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
    tokio::spawn(async move { axum::serve(l, link).await });
    let settings = LinkSettings {
        client_id: "magnus".into(),
        publishable_key: "pk".into(),
        client_secret: "link_secret".into(),
        redirect_uri: "http://127.0.0.1:9/cb".into(),
        api_url: base.clone(),
        login_url: base,
        test_mode: true,
    };
    s.k.vault.put("link_secret", "sk").unwrap();
    let old = Tokens { access: "old".into(), refresh: "rt".into(), expires_at: 0 };
    s.k.vault.put(&token_name("mo"), &serde_json::to_string(&old).unwrap()).unwrap();

    let (k, set) = (s.k.clone(), settings.clone());
    let refresh = tokio::spawn(async move { access(&k, &set, "mo").await.unwrap() });
    arrived.notified().await;
    revoke(&s.k, &settings, "mo").await.unwrap();
    go.notify_one();
    assert_eq!(refresh.await.unwrap(), None, "a refresh gave out a Link token after the sign-in ended");
    assert!(s.k.vault.get(&token_name("mo")).unwrap().is_none(), "a refresh wrote the Link sign-in back after it ended");
    // The old refresh token when it ended, then the one the refresh was given after it.
    assert_eq!(revoked.load(Ordering::SeqCst), 2);
    s.done().await;
}
