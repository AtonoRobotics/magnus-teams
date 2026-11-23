//! Pure peer shared projects + server admin isolation (2026-11-22). Any regular
//! user can start a shared project without needing a Team Admin grant, no
//! shared computer VM is auto-provisioned, collaborators can add peers and
//! leave, and only the creator can delete it. A Magnus server admin sees shared
//! projects only as names and counts, and can never read, join, or delete one
//! they are not a member of.

use super::tests::server_with;
use axum::Router;
use reqwest::Method;
use serde_json::json;

const LEE: &str = "[[people]]\nid = \"lee\"\nname = \"Lee\"\n";

#[tokio::test(flavor = "multi_thread")]
async fn any_user_starts_a_shared_project_without_shared_vm_and_admins_stay_out() {
    let s = server_with(LEE, |_| Router::new()).await;
    let (sam, ana, mo, lee) = (s.token("sam").await, s.token("ana").await, s.token("mo").await, s.token("lee").await);

    // Any regular member (mo) starts a shared project directly — no Team Admin gate.
    let (st, v) = s.post(&mo, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}")).await.1["role"], "owner");
    // No shared workspace computer VM is auto-provisioned.
    assert!(s.k.team_computer(&ws).is_none(), "a shared workspace computer VM was auto-provisioned");

    // Collaborators are added as peers (role defaults to member) and can add others.
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "lee" })).await.0, 200);
    assert_eq!(s.get(&lee, &format!("/v1/workspaces/{ws}/people")).await.0, 200, "a collaborator could not list people");
    assert_eq!(s.call(&lee, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 403, "a non-creator deleted the shared project");

    // The Magnus server admin sees it only by name and counts, and nothing in it.
    let (st, v) = s.get(&ana, "/v1/admin/workspaces").await;
    assert_eq!(st, 200, "{v}");
    let w = &v["workspaces"][0];
    assert_eq!((w["name"].as_str(), w["members"].as_u64(), w["channels"].as_u64()), (Some("Launch"), Some(2), Some(1)), "{v}");
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

    // The creator deletes the shared project.
    assert_eq!(s.call(&mo, Method::DELETE, &format!("/v1/workspaces/{ws}"), None).await.0, 200, "the creator could not delete it");
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}")).await.0, 404);
    s.done().await;
}
