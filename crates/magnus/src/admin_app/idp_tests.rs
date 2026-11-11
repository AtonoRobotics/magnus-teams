//! The identity provider set in Admin › Security (`identity_provider.rs`):
//! only an owner, only after a test sign-in through it as themselves, and
//! never in a way that leaves them without a way in.

use super::guard::mark;
use super::tests::server;
use axum::routing::{get, post};
use axum::Router;
use base64::Engine;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// A provider on this machine offering the device-code grant; it signs in
/// whoever `who` names.
async fn provider(who: Arc<Mutex<String>>) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let iss = format!("http://{}", l.local_addr().unwrap());
    let (i1, i2, i3) = (iss.clone(), iss.clone(), iss.clone());
    let app =
        Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || async move { axum::Json(json!({ "issuer": i1, "token_endpoint": format!("{i1}/token"), "device_authorization_endpoint": format!("{i1}/device") })) }),
            )
            .route(
                "/device",
                post(move || async move {
                    axum::Json(json!({ "device_code": "d1", "user_code": "WXYZ", "verification_uri": format!("{i2}/verify"), "expires_in": 600, "interval": 0 }))
                }),
            )
            .route(
                "/token",
                post(move || {
                    let (iss, who) = (i3.clone(), who.lock().unwrap().clone());
                    async move {
                        let b64 = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
                        let claims = json!({ "iss": iss, "aud": "magnus", "exp": Utc::now().timestamp() + 300, "preferred_username": who });
                        axum::Json(json!({ "id_token": format!("{}.{}.sig", b64(&json!({})), b64(&claims)) }))
                    }
                }),
            );
    tokio::spawn(async move { axum::serve(l, app).await });
    iss
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_switches_the_identity_provider_only_after_signing_in_through_it() {
    let s = server(|_| Router::new()).await;
    let who = Arc::new(Mutex::new("ana".to_string()));
    let iss = provider(who.clone()).await;
    let settings = json!({ "issuer": iss, "client_id": "magnus", "label": "Test ID" });
    let (sam, ana) = (s.token("sam").await, s.token("ana").await);
    mark(&sam, Utc::now());
    mark(&ana, Utc::now());
    let put = |t: String, b: Value| {
        let s = &s;
        async move { s.call(&t, reqwest::Method::PUT, "/v1/admin/identity-provider", Some(b)).await }
    };
    let method = || async { s.get("", "/v1/sign-in").await.1["method"].clone() };
    assert_eq!(method().await, "password");

    // An admin who is not an owner may not: the provider says who anyone is.
    assert_eq!(s.post(&ana, "/v1/admin/identity-provider/test", settings.clone()).await.0, 403);
    assert_eq!(put(ana.clone(), json!({ "provider": settings })).await.0, 403);
    // Settings are checked before anything is tried.
    let (st, v) = s.post(&sam, "/v1/admin/identity-provider/test", json!({ "issuer": iss, "client_id": "magnus", "status_admin_ca": "/etc/ca.pem" })).await;
    assert_eq!(st, 400, "{v}");
    let (st, v) = s.post(&sam, "/v1/admin/identity-provider/test", json!({ "issuer": "http://id.example/realms/m", "client_id": "magnus" })).await;
    assert_eq!(st, 400, "a plain-http issuer elsewhere was taken: {v}");
    let (st, v) = s.post(&sam, "/v1/admin/identity-provider/test", json!({ "issuer": iss, "client_id": "magnus", "status_credential": "missing" })).await;
    assert_eq!(st, 400, "a status secret not in the vault was taken: {v}");
    // No test, no switch.
    let (st, v) = put(sam.clone(), json!({ "provider": settings })).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("test_first")), "{v}");

    // A provider that signs the owner in as someone else fails the test.
    let (st, v) = s.post(&sam, "/v1/admin/identity-provider/test", settings.clone()).await;
    assert_eq!((st, v["user_code"].as_str()), (200, Some("WXYZ")), "{v}");
    let (_, v) = s.post(&sam, "/v1/admin/identity-provider/test/poll", json!({})).await;
    assert_eq!(v["status"], "failed", "{v}");
    assert_eq!(put(sam.clone(), json!({ "provider": settings })).await.0, 409, "switched after a failed test");

    // Signed in as themselves, the owner switches; people sign in through it.
    *who.lock().unwrap() = "sam".into();
    assert_eq!(s.post(&sam, "/v1/admin/identity-provider/test", settings.clone()).await.0, 200);
    let (_, v) = s.post(&sam, "/v1/admin/identity-provider/test/poll", json!({})).await;
    assert_eq!(v["status"], "passed", "{v}");
    let other = json!({ "issuer": iss, "client_id": "magnus", "label": "Another" });
    assert_eq!(put(sam.clone(), json!({ "provider": other })).await.0, 409, "settings other than those tested were taken");
    let (st, v) = put(sam.clone(), json!({ "provider": settings })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(method().await, "oidc");
    assert_eq!(s.get(&sam, "/v1/admin/sign-in-policy").await.1["identity_provider"], json!(iss));
    assert!(s.k.may_use_password("sam") && !s.k.may_use_password("mo"), "with a provider a password here is the owner's backup alone");
    let set = s.audited("identity_provider_set");
    assert_eq!(set.len(), 1, "the switch is not in the admin audit");
    assert_eq!(set[0]["after"]["issuer"], json!(iss));
    // A test is used once.
    assert_eq!(put(sam.clone(), json!({ "provider": settings })).await.0, 409);

    // Back to passwords kept here: the owner has one, so they keep a way in.
    assert_eq!(put(sam.clone(), json!({ "provider": null })).await.0, 200);
    assert_eq!(method().await, "password");
    assert!(s.k.may_use_password("mo"));
    s.done().await;
}
