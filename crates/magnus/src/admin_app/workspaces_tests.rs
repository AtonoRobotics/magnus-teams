//! Team Admin (the owner, 2026-11-11). sam is the server's owner, ana an
//! admin, mo a member, lee a member who will be a workspace admin. A Magnus
//! admin makes someone a Team Admin (the server's owner was made one when
//! the role came in) and sees workspaces only as names,
//! Team Admins and counts; they neither join, change nor delete one. Only a
//! Team Admin starts a workspace, and owns it; its Team Admins (its owner
//! and its admins) delete it.

use super::tests::{server_with, PASSWORD};
use axum::Router;
use reqwest::Method;
use serde_json::json;

const LEE: &str = "[[people]]\nid = \"lee\"\nname = \"Lee\"\n";

#[tokio::test(flavor = "multi_thread")]
async fn a_magnus_admin_makes_team_admins_and_stays_out_of_their_workspaces() {
    let s = server_with(LEE, |_| Router::new()).await;
    let (sam, ana, mo, lee) = (s.token("sam").await, s.token("ana").await, s.token("mo").await, s.token("lee").await);
    // Nobody starts a workspace without the role, a server admin included.
    for (who, t) in [("an admin", &ana), ("a member", &mo)] {
        assert_eq!(s.post(t, "/v1/workspaces", json!({ "name": "Launch" })).await.0, 403, "{who} started a workspace without being a Team Admin");
    }
    // The server's owner was made one when Team Admin came in.
    assert_eq!(s.get(&sam, "/v1/admin/standing").await.1["team_admin"], true, "the server's owner was not seeded as a Team Admin");
    assert_eq!(s.get(&mo, "/v1/admin/standing").await.1["team_admin"], false);
    let grant = "/v1/admin/people/mo/team-admin";
    assert_eq!(s.call(&mo, Method::PUT, grant, Some(json!({ "on": true }))).await.0, 403, "a member made themselves a Team Admin");
    let (st, v) = s.call(&ana, Method::PUT, grant, Some(json!({ "on": true }))).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "the grant went through without a step-up: {v}");
    assert_eq!(s.post(&ana, "/v1/admin/step-up", json!({ "password": PASSWORD })).await.0, 200);
    assert_eq!(s.call(&ana, Method::PUT, grant, Some(json!({ "on": true }))).await.0, 200);
    assert_eq!(s.audited("team_admin_set").len(), 1);
    assert_eq!(s.get(&mo, "/v1/admin/standing").await.1["team_admin"], true);

    // mo starts one and owns it, and makes lee an admin of it.
    let (st, v) = s.post(&mo, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}")).await.1["role"], "owner");
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "lee", "role": "admin" })).await.0, 200);

    // The Magnus admin sees it by name, Team Admins and counts, and nothing in it.
    let (st, v) = s.get(&ana, "/v1/admin/workspaces").await;
    assert_eq!(st, 200, "{v}");
    let w = &v["workspaces"][0];
    assert_eq!((w["name"].as_str(), w["members"].as_u64(), w["channels"].as_u64()), (Some("Launch"), Some(2), Some(1)), "{v}");
    assert_eq!(w["team_admins"].as_array().map(Vec::len), Some(2), "{v}");
    assert!(!v.to_string().contains("general"), "the admin list carries the workspace's channels: {v}");
    // ... and cannot join, change or delete it, nor read it.
    let members = format!("/v1/admin/workspaces/{ws}/members");
    let (st, _) = s.post(&ana, &members, json!({ "person": "ana", "role": "owner" })).await;
    assert!(st == 404 || st == 405, "the admin added themselves to a workspace: {st}");
    let (st, _) = s.call(&ana, Method::DELETE, &format!("/v1/admin/workspaces/{ws}"), None).await;
    assert!(st == 404 || st == 405, "the admin deleted a workspace: {st}");
    for t in [&ana, &sam] {
        assert_eq!(s.get(t, &format!("/v1/workspaces/{ws}")).await.0, 404, "someone outside the workspace read it");
        assert_eq!(s.call(t, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 404, "someone outside the workspace deleted it");
    }

    // Taking the role back stops mo starting another; Launch stays theirs.
    assert_eq!(s.call(&ana, Method::PUT, grant, Some(json!({ "on": false }))).await.0, 200);
    assert_eq!(s.post(&mo, "/v1/workspaces", json!({ "name": "Second" })).await.0, 403);
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}")).await.1["role"], "owner");

    // A workspace's Team Admin who is its admin, not its owner, deletes it.
    assert_eq!(s.call(&lee, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 200, "the workspace's admin could not delete it");
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}")).await.0, 404);
    s.done().await;
}
