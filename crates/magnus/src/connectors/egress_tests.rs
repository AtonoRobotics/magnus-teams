//! Every caller that reaches an address an app chose (its MCP address, its
//! sign-in metadata, its registration and token endpoints) goes through
//! `egress_client`: an address resolving to 127.0.0.1 and a redirect to a
//! private address are both refused when private networks are not allowed.

use super::discovery::{asks_sign_in, discover, own_name};
use super::sign_in::registration_request;
use crate::admin_app::tests::server_with;
use crate::apps::Mcp;
use crate::egress_client::tests::{by_name, counting, stand_in, STAND_IN, WAIT};
use crate::egress_client::Egress;
use axum::Router;
use serde_json::json;
use std::sync::atomic::Ordering;

fn refusing() -> Egress {
    Egress::new(false, WAIT, WAIT)
}

fn form() -> Vec<(&'static str, String)> {
    vec![("grant_type", "refresh_token".to_string()), ("refresh_token", "rt".to_string())]
}

#[tokio::test]
async fn every_caller_refuses_an_address_resolving_to_loopback() {
    let (base, hits) = counting("127.0.0.1", 200, None).await;
    let at = |path: &str| format!("{}{path}", by_name(&base));
    assert!(Mcp::new().unwrap().list_tools(&at("/mcp"), None).await.is_err(), "app rpc");
    assert!(asks_sign_in(&refusing(), &at("/mcp")).await.is_err(), "discovery: initialize");
    assert!(own_name(&refusing(), &at("/mcp")).await.is_none(), "discovery: the app's name");
    assert!(discover(&refusing(), &at("/mcp"), None).await.is_err(), "discovery: metadata");
    assert!(discover(&refusing(), "https://app.example/mcp", Some(at("/prm"))).await.is_err(), "discovery: the metadata address the app names");
    assert!(registration_request(&refusing(), &at("/register"), &json!({})).await.is_err(), "registration");
    assert!(crate::token_endpoint::post(&refusing(), &at("/token"), &form()).await.is_err(), "token");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "a request reached the loopback address");
    // Where private networks are allowed, each reaches it.
    let allowing = Egress::new(true, WAIT, WAIT);
    let _ = Mcp::new().unwrap().allowing_private(true).list_tools(&format!("{base}/mcp"), None).await;
    let _ = asks_sign_in(&allowing, &format!("{base}/mcp")).await;
    let _ = registration_request(&allowing, &format!("{base}/register"), &json!({})).await.unwrap();
    let _ = crate::token_endpoint::post(&allowing, &format!("{base}/token"), &form()).await.unwrap();
    assert!(hits.load(Ordering::SeqCst) >= 4, "{}", hits.load(Ordering::SeqCst));
}

#[tokio::test]
async fn no_caller_follows_a_redirect_to_a_private_address() {
    let (private, private_hits) = counting("127.0.0.1", 200, None).await;
    let (hop, hop_hits) = counting(STAND_IN, 307, Some(format!("{private}/inside"))).await;
    let at = |path: &str| format!("{hop}{path}");
    let e = stand_in();
    assert!(Mcp::new().unwrap().reaching(e).list_tools(&at("/mcp"), None).await.is_err(), "app rpc");
    assert_eq!(asks_sign_in(&e, &at("/mcp")).await.unwrap(), None, "discovery: initialize");
    assert!(discover(&e, &at("/mcp"), None).await.is_err(), "discovery: metadata");
    assert_eq!(registration_request(&e, &at("/register"), &json!({})).await.unwrap().status().as_u16(), 307, "registration");
    assert_eq!(crate::token_endpoint::post(&e, &at("/token"), &form()).await.unwrap().status().as_u16(), 307, "token");
    assert!(hop_hits.load(Ordering::SeqCst) >= 5);
    assert_eq!(private_hits.load(Ordering::SeqCst), 0, "a redirect to the private address was followed");
}

/// The kernel reaches an app's endpoints as its config says: here private
/// networks are not allowed, and a token endpoint and a registration
/// endpoint on 127.0.0.1 are never reached.
#[tokio::test(flavor = "multi_thread")]
async fn the_server_refuses_an_apps_private_endpoints_unless_its_config_allows_them() {
    const APP: &str =
        "[[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\ncredential = \"connector_crm\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n";
    let (base, hits) = counting("127.0.0.1", 200, None).await;
    let s = server_with(APP, |_| Router::new()).await;
    let stored = json!({ "access_token": "old", "refresh_token": "rt", "expires_at": 0, "lifetime": 3600,
                         "client": { "token_endpoint": format!("{base}/token"), "client_id": "magnus", "resource": "r" } });
    s.k.vault.put(&super::connection_name("crm", "mo"), &stored.to_string()).unwrap();
    let c = s.k.connector("crm").unwrap();
    assert!(s.k.connector_secret(&c, "mo").await.unwrap().is_none(), "a refresh at a private token endpoint");
    assert!(s.k.register_client("registered_client test", &format!("{base}/register"), "http://127.0.0.1:1/callback").await.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the server reached the app's private endpoint");
    s.done().await;
}
