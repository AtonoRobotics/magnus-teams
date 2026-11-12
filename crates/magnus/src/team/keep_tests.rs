//! What a member shares stays theirs: the team gets a copy that its admins
//! may delete, and the member's own is never touched (owner, 2026-11-11).
//! sam owns the workspace, mo is a member.

use super::agent::{held_dir, volume_dir};
use super::{Change, File};
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use axum::Router;
use reqwest::Method;
use serde_json::json;

async fn workspace(s: &Server) -> (String, String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    (ws, sam, s.token("mo").await)
}

/// A file the team agent kept back for mo, as `team_write` does for a run
/// that read mo's own data.
async fn hold(s: &Server, ws: &str, name: &str, text: &str) {
    let dir = held_dir(&s.k, ws, "mo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
    let file = File { name: name.into(), size: text.len() as u64, by: "mo".into() };
    s.k.emit("run:r", Ev::Team(Change::Held { workspace: ws.into(), person: "mo".into(), file })).await.unwrap();
}

/// The text of each file in mo's own library called `name`.
async fn in_library(s: &Server, name: &str) -> Vec<String> {
    use base64::Engine;
    let ids: Vec<String> = s.k.read(|st| st.files.values().filter(|f| f.person == "mo" && f.name == name).map(|f| f.id.clone()).collect());
    let rows = s.k.log.of_kinds(&["file_uploaded"], 500).await.unwrap();
    let seqs: Vec<i64> = rows.iter().map(|(seq, _, _, _)| *seq).collect();
    let bodies = s.k.log.contents(&seqs).await.unwrap();
    bodies
        .values()
        .filter(|b| b["id"].as_str().is_some_and(|id| ids.iter().any(|i| i == id)))
        .map(|b| String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b["data_b64"].as_str().unwrap()).unwrap()).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_keeps_what_was_held_for_them() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, _) = workspace(&s).await;
    hold(&s, &ws, "salary.txt", "mo's own figures").await;
    assert_eq!(s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}/members/mo"), None).await.0, 200);
    assert_eq!(in_library(&s, "salary.txt").await, vec!["mo's own figures".to_string()], "removing mo took their own file with them");
    assert!(!held_dir(&s.k, &ws, "mo").exists(), "the workspace still holds it");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_workspace_leaves_a_members_own_files_with_them() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, _) = workspace(&s).await;
    hold(&s, &ws, "salary.txt", "mo's own figures").await;
    assert_eq!(s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 200);
    assert_eq!(in_library(&s, "salary.txt").await, vec!["mo's own figures".to_string()], "deleting the workspace deleted mo's own file");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_deletes_the_shared_copy_only() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo) = workspace(&s).await;
    hold(&s, &ws, "plan.txt", "mo's plan").await;
    let (st, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/held/plan.txt/share"), json!({})).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = s.get(&sam, &format!("/v1/workspaces/{ws}/files/plan.txt")).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}/files/plan.txt"), None).await.0, 200, "the workspace's owner could not moderate");
    assert!(!volume_dir(&s.k, &ws).join("plan.txt").exists(), "the team's copy is still there");
    assert_eq!(in_library(&s, "plan.txt").await, vec!["mo's plan".to_string()], "deleting the team's copy deleted mo's own");
    s.done().await;
}

/// The workspace's owner takes a message down from a channel; a direct
/// conversation stays its people's, and a member moderates nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_moderates_a_channel_and_not_a_direct_conversation() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo) = workspace(&s).await;
    let general = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.iter().find(|(_, c)| c.name == "general").map(|(id, _)| id.clone()).unwrap());
    let say = |t: String, c: String, text: &'static str| {
        let (ws, s) = (ws.clone(), &s);
        async move { s.post(&t, &format!("/v1/workspaces/{ws}/channels/{c}/messages"), json!({ "text": text })).await.1["id"].as_str().unwrap().to_string() }
    };
    let gone = |c: &str, id: &str| format!("/v1/workspaces/{ws}/channels/{c}/messages/{id}");
    let by_sam = say(sam.clone(), general.clone(), "the launch is friday").await;
    assert_eq!(s.call(&mo, Method::DELETE, &gone(&general, &by_sam), None).await.0, 403, "a member deleted someone else's message");
    let by_mo = say(mo.clone(), general.clone(), "something off-colour").await;
    assert_eq!(s.call(&sam, Method::DELETE, &gone(&general, &by_mo), None).await.0, 200, "the workspace's owner could not moderate");
    let (_, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["sam"] })).await;
    let dm = v["id"].as_str().unwrap().to_string();
    let private = say(mo.clone(), dm.clone(), "just between us").await;
    assert_eq!(s.call(&sam, Method::DELETE, &gone(&dm, &private), None).await.0, 403, "an admin deleted mo's direct message");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_writes_over_someone_elses_file() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo) = workspace(&s).await;
    let http = reqwest::Client::new();
    let put = |t: String, text: &'static str| {
        let r = http.put(format!("{}/v1/workspaces/{ws}/files/notes.txt", s.base)).bearer_auth(t).body(text);
        async move { r.send().await.unwrap().status().as_u16() }
    };
    assert_eq!(put(mo.clone(), "mo's notes").await, 200);
    assert_eq!(put(sam.clone(), "sam's words").await, 409, "the workspace's owner wrote over mo's file");
    assert_eq!(std::fs::read_to_string(volume_dir(&s.k, &ws).join("notes.txt")).unwrap(), "mo's notes");
    assert_eq!(s.k.read(|st| st.team.workspace(&ws).unwrap().files["notes.txt"].by.clone()), "mo");
    assert_eq!(put(mo.clone(), "mo's notes, again").await, 200, "mo could not update their own file");
    s.done().await;
}
