//! Signing in refuses someone turned off here, whoever vouches for them
//! (decision `2026-11-23-one-person-is-turned-off.md`).

use super::provider_answered;
use crate::api::Transport;
use crate::identity::{Polled, Rights};

/// Someone turned off here but still on at the identity provider signed in
/// there: the sign-in cleared the hold on their work before their session
/// came back dead, so their routines and new work started again. The
/// sign-in is refused now before anything of it is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_provider_sign_in_by_someone_turned_off_leaves_them_off() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    s.k.turn_off_account("admin:ana", "mo").await.unwrap();
    assert!(s.k.refuse_new("mo", false).is_some());
    let done = Polled::Done { person: "mo".into(), app: true, rights: Rights { groups: None, roles: Some(vec!["admin".into()]), subject: Some("idp-mo".into()) } };
    let r = provider_answered(&s.k, Transport::Plain, "Example ID".into(), done, None).await;
    assert_eq!(r.status(), axum::http::StatusCode::FORBIDDEN, "an account turned off here signed in");
    assert!(s.k.refuse_new("mo", false).is_some(), "the sign-in cleared the hold on their work");
    assert!(s.k.read(|st| st.identity.is_turned_off("mo")), "the sign-in cleared the turned-off mark");
    assert!(s.k.read(|st| st.identity.subject("mo").is_none() && st.identity.provider_rights("mo").is_none()), "the refused sign-in was recorded");
    assert!(!s.k.accounts.signed_in().await.contains("mo"), "a session was opened for them");
    assert_eq!(s.k.accounts.status("mo").await, crate::accounts::Status::Disabled);
    s.done().await;
}
