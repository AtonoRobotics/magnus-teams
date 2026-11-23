//! A shared project's own sign-ins end: with the project when it is
//! deleted, and by its admin while their app is turned off. sam owns the
//! project, mo is a member.

use super::owner_key;
use crate::admin_app::tests::{server_with, Server};
use crate::connectors::connection_name;
use axum::Router;
use reqwest::Method;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const APPS: &str =
    "[[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\ncredential = \"connector_crm\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n";

/// An app's revocation endpoint, counting the tokens revoked there.
async fn revoker() -> (String, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let app = Router::new().route(
        "/revoke",
        axum::routing::post(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::OK
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/revoke", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, n)
}

fn signed_in(s: &Server, owner: &str, revoke_at: &str) {
    let t = json!({ "access_token": "at", "refresh_token": "rt", "client": { "token_endpoint": "http://127.0.0.1:9/token", "revocation_endpoint": revoke_at, "client_id": "magnus", "resource": "r" } });
    s.k.vault.put(&connection_name("crm", owner), &t.to_string()).unwrap();
}

fn holds(s: &Server, owner: &str) -> bool {
    s.k.vault.get(&connection_name("crm", owner)).unwrap().is_some()
}

/// A project with crm granted to every role, and the owner's token.
async fn project(s: &Server) -> (String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/connectors"), json!({ "app": "crm", "roles": ["owner", "admin", "member"] })).await.0, 200);
    (ws, sam)
}

/// Deleting a project revokes its own sign-ins at the app and deletes
/// them; a member's own sign-in to the same app stays.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_project_revokes_its_sign_ins() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (revoke_at, revoked) = revoker().await;
    let (ws, sam) = project(&s).await;
    signed_in(&s, &owner_key(&ws), &revoke_at);
    signed_in(&s, "mo", &revoke_at);
    let (st, v) = s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await;
    assert_eq!(st, 200, "{v}");
    assert!(!holds(&s, &owner_key(&ws)), "a deleted project's sign-in stayed in the vault");
    assert_eq!(revoked.load(Ordering::SeqCst), 2, "the refresh and access tokens, revoked at the app");
    assert!(holds(&s, "mo"), "a member's own sign-in ended with the project");
    s.done().await;
}

/// Turned off, an app keeps its sign-ins: the project's admin can still
/// end the project's, so it does not quietly work again when the app is
/// turned back on.
#[tokio::test(flavor = "multi_thread")]
async fn a_turned_off_apps_project_sign_in_can_still_be_ended() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (revoke_at, revoked) = revoker().await;
    let (ws, sam) = project(&s).await;
    signed_in(&s, &owner_key(&ws), &revoke_at);
    s.k.connector_settings("person:sam", "crm", Some(false), None, None).await.unwrap();
    assert!(s.k.connector("crm").is_none());
    let (st, v) = s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}/connectors/crm/connection"), None).await;
    assert_eq!(st, 200, "{v}");
    assert!(!holds(&s, &owner_key(&ws)));
    assert_eq!(revoked.load(Ordering::SeqCst), 2);
    s.k.connector_settings("person:sam", "crm", Some(true), None, None).await.unwrap();
    assert!(!s.k.workspace_signed_in(&ws, &s.k.connector("crm").unwrap()), "the ended sign-in works again");
    s.done().await;
}

/// The admin console counts people signed in to an app: a project's own
/// sign-in is not a person.
#[tokio::test(flavor = "multi_thread")]
async fn a_projects_sign_in_is_not_counted_as_a_person() {
    let s = server_with(APPS, |_| Router::new()).await;
    signed_in(&s, &owner_key("w1"), "http://127.0.0.1:9/revoke");
    signed_in(&s, "mo", "http://127.0.0.1:9/revoke");
    let crm = s.k.admin_connectors().into_iter().find(|c| c["name"] == "crm").unwrap();
    assert_eq!(crm["signed_in"], 1, "{crm}");
    s.done().await;
}
