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
