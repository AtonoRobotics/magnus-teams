//! Admin › People › Workspaces: a server admin deletes a workspace or
//! changes who is in it, audited and after a step-up, and the workspace's
//! own rules still hold. ana is a server admin in no workspace.

use super::tests::{server, PASSWORD};
use axum::Router;
use reqwest::Method;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_server_admin_deletes_a_workspace_after_a_step_up_and_it_is_audited() {
    let s = server(|_| Router::new()).await;
    let (sam, mo, ana) = (s.token("sam").await, s.token("mo").await, s.token("ana").await);
    let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "admin" })).await.0, 200);
    assert_eq!(s.call(&mo, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 403, "a workspace admin deleted the workspace");
    assert_eq!(s.call(&mo, Method::DELETE, &format!("/v1/admin/workspaces/{ws}"), None).await.0, 403, "a member reached the admin route");

    let (st, v) = s.get(&ana, "/v1/admin/workspaces").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["workspaces"][0]["name"], "Launch");
    let path = format!("/v1/admin/workspaces/{ws}");
    let (st, v) = s.call(&ana, Method::DELETE, &path, None).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "a delete went through without a step-up: {v}");
    assert_eq!(s.post(&ana, "/v1/admin/step-up", json!({ "password": PASSWORD })).await.0, 200);
    let (st, v) = s.call(&ana, Method::DELETE, &path, None).await;
    assert_eq!(st, 200, "{v}");
    let entry = s.audited("workspace_deleted");
    assert_eq!(entry.len(), 1);
    assert_eq!((entry[0]["admin"].as_str(), entry[0]["before"]["name"].as_str()), (Some("ana"), Some("Launch")));
    for t in [&sam, &mo] {
        assert_eq!(s.get(t, &format!("/v1/workspaces/{ws}/home")).await.0, 404, "a member still reads a deleted workspace");
    }
    assert!(s.get(&sam, "/v1/workspaces").await.1.as_array().unwrap().is_empty());
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_admin_adds_and_removes_members_but_the_workspace_keeps_its_owner() {
    let s = server(|_| Router::new()).await;
    let (sam, mo, ana) = (s.token("sam").await, s.token("mo").await, s.token("ana").await);
    let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&ana, "/v1/admin/step-up", json!({ "password": PASSWORD })).await.0, 200);
    let members = format!("/v1/admin/workspaces/{ws}/members");
    assert_eq!(s.post(&ana, &members, json!({ "person": "mo", "role": "member" })).await.0, 200);
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}/home")).await.0, 200, "the person added cannot read it");
    assert_eq!(s.audited("workspace_member_set").len(), 1);
    assert_eq!(s.call(&ana, Method::DELETE, &format!("{members}/sam"), None).await.0, 403, "the only owner was removed");
    assert_eq!(s.call(&ana, Method::DELETE, &format!("{members}/mo"), None).await.0, 200);
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}/home")).await.0, 404, "the person removed still reads it");
    assert_eq!(s.audited("workspace_member_removed").len(), 1);
    s.done().await;
}
