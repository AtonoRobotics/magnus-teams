//! Sharing a personal project is one call, and any member renames the shared
//! project (2026-11-28-projects-without-hoops). sam owns the server; mo is a
//! server member.

use crate::admin_app::tests::server_with;
use axum::Router;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_project_is_shared_in_one_call_and_any_member_renames_it() {
    let s = server_with("", |_| Router::new()).await;
    let (sam, mo) = (s.token("sam").await, s.token("mo").await);
    let (st, v) = s.post(&sam, "/v1/projects", json!({ "name": "Launch", "instructions": "Be brief." })).await;
    assert_eq!(st, 201, "{v}");
    let project = v["id"].as_str().unwrap().to_string();

    assert_eq!(s.post(&mo, &format!("/v1/projects/{project}/share"), json!({ "people": [] })).await.0, 404, "mo shared sam's project");
    assert_eq!(s.post(&sam, &format!("/v1/projects/{project}/share"), json!({ "people": ["nobody"] })).await.0, 404, "a share named no one real");
    let (st, v) = s.post(&sam, &format!("/v1/projects/{project}/share"), json!({ "people": ["mo"] })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["workspace"].as_str().unwrap().to_string();

    let (st, home) = s.get(&mo, &format!("/v1/workspaces/{ws}")).await;
    assert_eq!(st, 200, "mo was not added: {home}");
    assert_eq!(home["name"], "Launch");
    assert_eq!(home["members"]["mo"], "member");
    assert_eq!(home["members"]["sam"], "owner");
    assert_eq!(home["channels"].as_array().map(Vec::len), Some(1), "the shared project has no conversation to talk in: {home}");
    let instructions = s.k.read(|st| st.team.workspace(&ws).and_then(|w| w.instructions.clone()));
    assert_eq!(instructions.as_deref(), Some("Be brief."), "the instructions did not come along");

    let (st, v) = s.call(&mo, reqwest::Method::PATCH, &format!("/v1/workspaces/{ws}"), Some(json!({ "name": "Launch v2" }))).await;
    assert_eq!(st, 200, "a member could not rename: {v}");
    assert_eq!(s.get(&sam, &format!("/v1/workspaces/{ws}")).await.1["name"], "Launch v2");
    assert_eq!(s.call(&mo, reqwest::Method::PATCH, &format!("/v1/workspaces/{ws}"), Some(json!({ "name": " " }))).await.0, 400, "a blank name was taken");
    s.done().await;
}
