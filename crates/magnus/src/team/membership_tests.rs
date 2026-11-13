//! Who is in a workspace: an owner or admin adds and removes, anyone
//! leaves, nobody touches a role above their own, and a workspace keeps an
//! owner. sam owns the workspace; mo, ana and lee are people on the server.

use super::membership::refuse;
use super::{Change, Ledger, Role};
use crate::admin_app::tests::server_with;
use axum::Router;
use reqwest::Method;
use serde_json::json;

fn ledger(members: &[(&str, Role)]) -> Ledger {
    let mut l = Ledger::default();
    l.apply(&Change::Created { workspace: "w".into(), name: "T".into(), owner: "sam".into() });
    for (p, r) in members {
        l.apply(&Change::Member { workspace: "w".into(), person: p.to_string(), role: *r });
    }
    l
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_owner_departures_leave_one_owner() {
    let s = server_with("", |_| Router::new()).await;
    let (sam, mo) = (s.token("sam").await, s.token("mo").await);
    let ws = s.post(&sam, "/v1/workspaces", json!({"name":"W"})).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({"person":"mo","role":"owner"})).await.0, 200);
    let a = format!("/v1/workspaces/{ws}/members/sam");
    let b = format!("/v1/workspaces/{ws}/members/mo");
    let (a, b) = tokio::join!(s.call(&sam, Method::DELETE, &a, None), s.call(&mo, Method::DELETE, &b, None));
    assert_eq!(usize::from(a.0 == 200) + usize::from(b.0 == 200), 1);
    assert_eq!(s.k.read(|s| s.team.workspace(&ws).unwrap().members.values().filter(|r| **r == Role::Owner).count()), 1);
    s.done().await;
}

#[test]
fn a_workspace_keeps_an_owner_and_nobody_reaches_above_their_role() {
    let l = ledger(&[("mo", Role::Admin), ("ana", Role::Member)]);
    let w = l.workspace("w").unwrap();
    assert!(refuse(w, "mo", Role::Admin, "sam", None).is_some(), "an admin removed the owner");
    assert!(refuse(w, "mo", Role::Admin, "sam", Some(Role::Member)).is_some(), "an admin demoted the owner");
    assert!(refuse(w, "sam", Role::Owner, "sam", None).is_some(), "the only owner left");
    assert!(refuse(w, "sam", Role::Owner, "sam", Some(Role::Admin)).is_some(), "the only owner stepped down");
    assert!(refuse(w, "mo", Role::Admin, "ana", Some(Role::Owner)).is_some(), "an admin made an owner");
    assert_eq!(refuse(w, "mo", Role::Admin, "ana", None), None);
    assert_eq!(refuse(w, "ana", Role::Member, "ana", None), None, "a member could not leave");
    // Ownership handed on first, the old owner may go.
    let l = ledger(&[("mo", Role::Owner)]);
    assert_eq!(refuse(l.workspace("w").unwrap(), "sam", Role::Owner, "sam", None), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn owners_and_admins_add_and_remove_and_anyone_leaves() {
    let s = server_with("", |_| Router::new()).await;
    let (sam, mo, ana) = (s.token("sam").await, s.token("mo").await, s.token("ana").await);
    let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
    let base = format!("/v1/workspaces/{ws}");

    let (st, people) = s.get(&sam, &format!("{base}/people")).await;
    assert_eq!(st, 200, "{people}");
    let ids: Vec<&str> = people.as_array().unwrap().iter().filter_map(|p| p["id"].as_str()).collect();
    assert!(ids.contains(&"mo") && ids.contains(&"lee") && !ids.contains(&"sam"), "{people}");
    assert!(people.to_string().find('@').is_none(), "an address reached the list: {people}");

    assert_eq!(s.post(&sam, &format!("{base}/members"), json!({ "person": "mo", "role": "admin" })).await.0, 200);
    assert_eq!(s.post(&sam, &format!("{base}/members"), json!({ "person": "ana", "role": "member" })).await.0, 200);
    assert_eq!(s.get(&ana, &format!("{base}/people")).await.0, 403, "a member listed the server's people");
    assert_eq!(s.call(&ana, Method::DELETE, &format!("{base}/members/mo"), None).await.0, 403, "a member removed someone");
    assert_eq!(s.call(&mo, Method::DELETE, &format!("{base}/members/sam"), None).await.0, 403, "an admin removed the owner");
    assert_eq!(s.call(&sam, Method::DELETE, &format!("{base}/members/sam"), None).await.0, 403, "the only owner left");

    let (st, v) = s.call(&ana, Method::DELETE, &format!("{base}/members/ana"), None).await;
    assert_eq!(st, 200, "a member could not leave: {v}");
    assert_eq!(s.get(&ana, &format!("{base}/home")).await.0, 404, "who left still reads the workspace");

    assert_eq!(s.call(&mo, Method::DELETE, &format!("{base}/members/mo"), None).await.0, 200);
    assert_eq!(s.post(&sam, &format!("{base}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    assert_eq!(s.call(&sam, Method::DELETE, &format!("{base}/members/mo"), None).await.0, 200);
    assert_eq!(s.get(&mo, &format!("{base}/home")).await.0, 404, "a removed member still reads the workspace");

    // Ownership handed on, the old owner may go.
    assert_eq!(s.post(&sam, &format!("{base}/members"), json!({ "person": "mo", "role": "owner" })).await.0, 200);
    assert_eq!(s.call(&sam, Method::DELETE, &format!("{base}/members/sam"), None).await.0, 200);
    assert_eq!(s.k.read(|st| st.team.role(&ws, "mo")), Some(Role::Owner));
    s.done().await;
}

/// "Removing a member revokes their grants, ends their running tasks and
/// leaves team files in place" (spec, Team shared workspace › Membership
/// changes): the team agent's work for them stops at once, what they put in
/// the workspace stays, and they can ask the team agent for nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_member_ends_their_running_tasks_and_leaves_team_files() {
    let s = server_with("", |_| Router::new()).await;
    let (sam, mo) = (s.token("sam").await, s.token("mo").await);
    let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
    let base = format!("/v1/workspaces/{ws}");
    assert_eq!(s.post(&sam, &format!("{base}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    let put = reqwest::Client::new().put(format!("{}{base}/files/plan.txt", s.base)).bearer_auth(&mo).body("the plan").send().await.unwrap();
    assert_eq!(put.status().as_u16(), 200, "mo could not add a team file");
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.team_request(&ws, &channel, None, "mo", "draft the launch note".into()).await.unwrap();
    let run_of = || s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).map(|r| r.status.terminal()));

    assert_eq!(s.call(&sam, Method::DELETE, &format!("{base}/members/mo"), None).await.0, 200);
    assert_eq!(run_of(), Some(true), "mo's team task still runs after mo was removed");
    assert!(s.k.read(|st| st.team.runs_by(&ws, "mo").iter().all(|r| st.tasks.get(&r.task).and_then(|t| st.runs.get(&t.run_id)).is_none_or(|run| run.status.terminal()))));
    assert!(s.k.read(|st| st.team.workspace(&ws).unwrap().files.contains_key("plan.txt")), "mo's team file left with mo");
    assert_eq!(s.get(&sam, &format!("{base}/files/plan.txt")).await.0, 200, "the team cannot read the file mo left");
    assert_eq!(s.get(&mo, &format!("{base}/files/plan.txt")).await.0, 404, "a removed member still reads a team file");
    assert_eq!(s.post(&mo, &format!("{base}/channels/{channel}/messages"), json!({ "text": "@Magnus one more thing" })).await.0, 404, "a removed member still asks the team agent");
    s.done().await;
}
