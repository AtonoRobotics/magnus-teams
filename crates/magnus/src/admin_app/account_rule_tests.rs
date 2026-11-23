//! One rule for changing someone's account (decision
//! `2026-11-23-one-rule-for-changing-an-account.md`), against a kernel
//! booted on a fresh database (the harness in `tests.rs`): sam owns the
//! server, ana is an admin by the config, lee by the directory, and mo is a
//! member.

use super::spec_tests::stepped;
use super::tests::server;
use crate::accounts::Status;
use axum::Router;
use serde_json::json;

/// An admin set an owner's recovery address to their own, then asked for a
/// reset and took the account; the same PATCH turned an owner off, which
/// POST /disable refuses. Both are refused now, and nothing changes.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_changes_no_owners_email_or_status() {
    let s = server(|_| Router::new()).await;
    let (sam, ana) = (stepped(&s, "sam").await, stepped(&s, "ana").await);
    // mo owns the server too, so sam is not the last owner and only the
    // account rule stands in the way.
    assert_eq!(s.call(&sam, reqwest::Method::PATCH, "/v1/admin/people/mo", Some(json!({ "role": "owner" }))).await.0, 204);
    let (st, v) = s.call(&ana, reqwest::Method::PATCH, "/v1/admin/people/sam", Some(json!({ "email": "ana@example.com" }))).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "an admin set an owner's recovery address: {v}");
    assert_eq!(s.k.accounts.local_person("sam").and_then(|p| p.email), None, "the owner's recovery address changed");
    assert!(s.k.accounts.request_reset("ana@example.com").await.unwrap().is_none(), "the admin's address recovers the owner");
    let (st, v) = s.call(&ana, reqwest::Method::PATCH, "/v1/admin/people/mo", Some(json!({ "status": "disabled" }))).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "an admin turned an owner off: {v}");
    assert_eq!(s.k.accounts.status("mo").await, Status::Active, "the owner was turned off");
    let (st, v) = s.post(&ana, "/v1/admin/people/mo/disable", json!({})).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "{v}");
    // The owner may, and an admin may for anyone who is not an owner.
    assert_eq!(s.call(&sam, reqwest::Method::PATCH, "/v1/admin/people/mo", Some(json!({ "email": "mo@example.com" }))).await.0, 204);
    assert_eq!(s.call(&ana, reqwest::Method::PATCH, "/v1/admin/people/lee", Some(json!({ "email": "lee@example.com" }))).await.0, 204);
    s.done().await;
}

/// The rule is the operation's own, so a caller that skips the Admin
/// section's routes is held to it too: someone who may not act on a person
/// sets no recovery address of theirs, changes no role, and erases nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_non_owner_sets_no_recovery_address_of_someone_they_may_not_act_on() {
    let s = server(|_| Router::new()).await;
    let e = s.k.change_account_email("admin:ana", "sam", Some("ana@example.com")).await.unwrap_err();
    assert_eq!(e.1, "owner_only", "an admin set the owner's recovery address");
    let e = s.k.change_account_email("person:mo", "lee", Some("mo@example.com")).await.unwrap_err();
    assert_eq!(e.1, "forbidden", "a member set an admin's recovery address");
    assert_eq!(s.k.accounts.local_person("lee").and_then(|p| p.email), None);
    assert_eq!(s.k.set_server_role("person:mo", "mo", "admin").await.unwrap_err().1, "forbidden", "a member made themselves an admin");
    assert_eq!(s.k.erase_person("admin:ana", "sam").await.unwrap_err().0, axum::http::StatusCode::FORBIDDEN, "an admin erased the owner");
    assert_eq!(s.k.renew_invitation("admin:ana", "sam").await.unwrap_err().1, "owner_only");
    s.k.change_account_email("admin:sam", "lee", Some("lee@example.com")).await.unwrap();
    s.k.change_account_email("person:mo", "mo", Some("mo@example.com")).await.unwrap();
    s.done().await;
}
