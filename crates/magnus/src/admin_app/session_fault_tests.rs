//! A database fault is not a sign-out
//! (`decisions/2026-11-14-a-database-fault-is-not-a-sign-out.md`): a
//! session the store cannot read answers 503 with an honest message, never
//! the 401 that makes the app drop the session, and the session stands once
//! the store answers again.

use super::tests::server;
use crate::api_error::SESSION_UNREAD;
use axum::Router;

/// While the sessions table cannot be read, a signed-in request answers 503
/// `{"error": ...}`, not 401 "sign in again"; when it can, the same token
/// works. Before, any store error read as no session, and the app signed
/// the person out.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_the_store_cannot_read_is_503_and_stands() {
    let s = server(|_| Router::new()).await;
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/me").await.0, 200);

    let db = s.store().await;
    db.batch_execute("ALTER TABLE public.account_sessions RENAME TO account_sessions_away").await.unwrap();
    let (st, v) = s.get(&mo, "/v1/me").await;
    assert_eq!(st, 503, "a store that cannot answer read as a signed-out person: {v}");
    assert_eq!(v["error"].as_str(), Some(SESSION_UNREAD), "{v}");
    let (st, v) = s.post(&mo, "/v1/inbox/opened", serde_json::json!({ "ids": [] })).await;
    assert_eq!(st, 503, "a change read the fault as a sign-out: {v}");

    db.batch_execute("ALTER TABLE public.account_sessions_away RENAME TO account_sessions").await.unwrap();
    assert_eq!(s.get(&mo, "/v1/me").await.0, 200, "the session did not stand once the store answered");
    // A token with no session is still a sign-out.
    assert_eq!(s.get("no-such-token", "/v1/me").await.0, 401);
    s.done().await;
}
