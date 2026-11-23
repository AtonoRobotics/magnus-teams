//! A change to how someone signs in takes their current password and ends
//! every other session of theirs (decision
//! `2026-11-23-credential-changes-end-other-sessions.md`), against a kernel
//! booted on a fresh database (the harness in `admin_app::tests`).

use crate::admin_app::tests::{server_with, Server, PASSWORD};
use reqwest::Method;
use serde_json::json;

fn code(secret: &str) -> String {
    let key = crate::totp::base32_decode(secret).unwrap();
    crate::totp::generate(&key, (chrono::Utc::now().timestamp() / 30) as u64, 6)
}

async fn live(s: &Server, token: &str) -> bool {
    s.get(token, "/v1/account/totp").await.0 == 200
}

/// A first authenticator could be enrolled with a session alone, so a
/// leftover browser session could lock the real person out; adding or
/// removing one ended no other session. Both take the current password now,
/// and both sign out every other device while this one stays.
#[tokio::test(flavor = "multi_thread")]
async fn adding_or_removing_an_authenticator_takes_the_password_and_ends_other_sessions() {
    let s = server_with("", |_| axum::Router::new()).await;
    let (here, elsewhere) = (s.token("mo").await, s.token("mo").await);
    let (st, v) = s.post(&here, "/v1/account/totp/begin", json!({})).await;
    assert_eq!(st, 200, "{v}");
    let secret = v["secret"].as_str().unwrap().to_string();
    let (st, v) = s.post(&here, "/v1/account/totp/confirm", json!({ "code": code(&secret) })).await;
    assert_eq!(st, 403, "a first authenticator was enrolled with a session alone: {v}");
    assert!(!s.k.accounts.has_totp("mo").await);
    assert!(live(&s, &elsewhere).await, "a refused change signed out another device");
    let (st, v) = s.post(&here, "/v1/account/totp/confirm", json!({ "code": code(&secret), "current_password": PASSWORD })).await;
    assert_eq!((st, v["signed_out"].as_u64()), (200, Some(1)), "{v}");
    assert!(s.k.accounts.has_totp("mo").await);
    assert!(!live(&s, &elsewhere).await, "enrolling an authenticator left another session signed in");
    assert!(live(&s, &here).await, "enrolling signed out the session that did it");

    let elsewhere = s.token("mo").await;
    let (st, v) = s.call(&here, Method::DELETE, "/v1/account/totp", Some(json!({ "current_password": PASSWORD }))).await;
    assert_eq!((st, v["signed_out"].as_u64()), (200, Some(1)), "{v}");
    assert!(!s.k.accounts.has_totp("mo").await);
    assert!(!live(&s, &elsewhere).await, "removing the authenticator left another session signed in");
    assert!(live(&s, &here).await);
    s.done().await;
}
