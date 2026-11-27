//! One reader for server roles and one "would this leave no enabled owner
//! or admin" (decision `2026-11-23-one-reader-for-server-roles.md`),
//! against a kernel booted on a fresh database (the harness in `tests.rs`):
//! sam owns the server, ana is an admin by the config, lee by the
//! directory, and mo is a member.

use super::spec_tests::stepped;
use super::tests::{server, Server};
use crate::account_removal::TeamWork;
use crate::accounts::Status;
use crate::events::Ev;
use crate::people::Authority;
use crate::team::{Change, Role};
use axum::Router;
use serde_json::json;

/// A workspace `owner` owns, with mo in it.
async fn workspace(s: &Server, ws: &str, owner: &str) {
    s.k.emit(&format!("person:{owner}"), Ev::Team(Change::Created { workspace: ws.into(), name: ws.into(), owner: owner.into() })).await.unwrap();
    s.k.emit(&format!("person:{owner}"), Ev::Team(Change::Member { workspace: ws.into(), person: "mo".into(), role: Role::Member })).await.unwrap();
}

fn owner_of(s: &Server, ws: &str) -> Option<String> {
    s.k.read(|st| st.team.workspace(ws).and_then(|w| w.members.iter().find(|(_, r)| **r == Role::Owner).map(|(p, _)| p.clone())))
}

/// The last-admin guard counted the directory alone: with a config owner
/// and one admin in the directory, the owner could neither turn that admin
/// off, demote nor remove them. It counts every place now.
#[tokio::test(flavor = "multi_thread")]
async fn an_owner_turns_off_demotes_and_removes_the_directorys_only_admin() {
    let s = server(|_| Router::new()).await;
    let sam = stepped(&s, "sam").await;
    let (st, v) = s.call(&sam, reqwest::Method::PATCH, "/v1/admin/people/lee", Some(json!({ "status": "disabled" }))).await;
    assert_eq!(st, 204, "the owner could not turn off the directory's only admin: {v}");
    assert_eq!(s.post(&sam, "/v1/admin/people/lee/enable", json!({})).await.0, 204);
    let (st, v) = s.call(&sam, reqwest::Method::PATCH, "/v1/admin/people/lee", Some(json!({ "role": "member" }))).await;
    assert_eq!(st, 204, "the owner could not demote the directory's only admin: {v}");
    s.k.set_server_role("admin:sam", "lee", "admin").await.unwrap();
    let (st, v) = s.call(&sam, reqwest::Method::DELETE, "/v1/admin/people/lee", Some(json!({ "projects": "delete" }))).await;
    assert_eq!(st, 200, "the owner could not remove the directory's only admin: {v}");
    assert!(s.k.person("lee").is_none(), "lee is still here");
    s.done().await;
}

/// A removal deleted the person's workspaces and moved their projects, and
/// only then was refused as the last admin's: the team work was gone and
/// the person stayed. Everything that refuses is asked first now.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_removal_changes_nothing() {
    let s = server(|_| Router::new()).await;
    workspace(&s, "sams", "sam").await;
    workspace(&s, "lees", "lee").await;
    let e = s.k.remove_account("console", "sam", TeamWork::Delete).await.unwrap_err();
    assert_eq!(e.1, "last_owner", "the last owner was removed");
    assert_eq!(owner_of(&s, "sams").as_deref(), Some("sam"), "a refused removal passed on the workspace");
    assert!(s.k.person("sam").is_some() && s.k.accounts.has_password("sam").await && s.k.accounts.status("sam").await == Status::Active, "a refused removal changed sam");
    // ana may not remove an owner: refused, with lee's workspace untouched
    // until a removal that is allowed passes it on.
    s.k.set_server_role("admin:sam", "lee", "owner").await.unwrap();
    assert_eq!(s.k.remove_account("admin:ana", "lee", TeamWork::Transfer("mo".into())).await.unwrap_err().1, "owner_only");
    assert_eq!(owner_of(&s, "lees").as_deref(), Some("lee"), "a refused removal passed on the workspace");
    s.k.remove_account("admin:sam", "lee", TeamWork::Transfer("mo".into())).await.unwrap();
    assert_eq!(owner_of(&s, "lees").as_deref(), Some("mo"));
    s.done().await;
}

/// The last directory admin deleting their own account erased everything
/// and answered deleted, but the refusal to remove the account was
/// swallowed: it stayed, with its password and admin role. Self-deletion
/// now removes the account, or refuses with nothing erased.
#[tokio::test(flavor = "multi_thread")]
async fn self_deletion_removes_the_account_or_refuses() {
    let s = server(|_| Router::new()).await;
    workspace(&s, "sams", "sam").await;
    let sam = s.token("sam").await;
    let (st, v) = s.call(&sam, reqwest::Method::DELETE, "/v1/account", Some(json!({ "confirm": "DELETE" }))).await;
    assert_eq!(st, 409, "the last owner deleted their own account: {v}");
    assert_eq!(owner_of(&s, "sams").as_deref(), Some("sam"), "a refused self-deletion erased their team work");
    let lee = s.token("lee").await;
    let (st, v) = s.call(&lee, reqwest::Method::DELETE, "/v1/account", Some(json!({ "confirm": "DELETE" }))).await;
    assert_eq!((st, v["deleted"].as_bool()), (200, Some(true)), "{v}");
    assert!(!s.k.accounts.is_local_fresh("lee").await, "the account survived its own deletion");
    assert!(!s.k.accounts.has_password("lee").await, "the password survived the account's deletion");
    assert!(s.k.person("lee").is_none());
    s.done().await;
}

/// Someone demoted at the identity provider stayed a server admin when a
/// role row had ever been written for them here. Where the provider names
/// the roles, it alone says.
#[tokio::test(flavor = "multi_thread")]
async fn a_provider_demotion_takes_effect() {
    let s = server(|_| Router::new()).await;
    let lee = s.k.person("lee").unwrap();
    assert_eq!(s.k.authority(&lee).await, Authority::Admin);
    s.k.emit("person:lee", Ev::RightsFromProvider(crate::events::RightsFromProviderPayload { person: "lee".into(), groups: None, roles: Some(vec!["member".into()]) }))
        .await
        .unwrap();
    let lee = s.k.person("lee").unwrap();
    assert_eq!(s.k.authority(&lee).await, Authority::Member, "a demotion at the provider did not take");
    let t = s.token("lee").await;
    assert_eq!(s.get(&t, "/v1/admin/standing").await.1["admin"], false, "the demoted admin still administers");
    s.k.emit("person:lee", Ev::RightsFromProvider(crate::events::RightsFromProviderPayload { person: "lee".into(), groups: None, roles: Some(vec!["admin".into()]) }))
        .await
        .unwrap();
    assert_eq!(s.k.authority(&s.k.person("lee").unwrap()).await, Authority::Admin);
    s.done().await;
}

/// The owner's roles route counted a turned-off owner as one who keeps the
/// server owned, so the last owner who is on could step down.
#[tokio::test(flavor = "multi_thread")]
async fn the_last_enabled_owner_cannot_be_demoted() {
    let s = server(|_| Router::new()).await;
    s.k.set_server_role("admin:sam", "mo", "owner").await.unwrap();
    s.k.turn_off_account("admin:sam", "mo").await.unwrap();
    let sam = stepped(&s, "sam").await;
    let (st, v) = s.call(&sam, reqwest::Method::PUT, "/v1/people/sam/roles", Some(json!({ "roles": ["admin"] }))).await;
    assert_eq!(st, 409, "the last owner who is on stepped down beside a turned-off one: {v}");
    assert!(crate::people::rights(&s.k.person("sam").unwrap()).is_server_owner(), "sam is no longer an owner");
    s.done().await;
}
