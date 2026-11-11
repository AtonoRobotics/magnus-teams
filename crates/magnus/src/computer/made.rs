//! A person's computer exists from the moment their account does (spec rev
//! 142). Accounts made by the server make it there and then; `magnus person
//! add` writes the directory from outside the server, so it tells a running
//! server over its control socket, and the server makes the computer at
//! once. With the server stopped, its start makes it.

use super::Owner;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// As long as the command line waits for the server to answer.
const TELL_WAIT_SECONDS: u64 = 30;

/// `POST /v1/people/{person}/computer` on the control socket: the person's
/// computer, made now if they have none.
pub(crate) async fn make_for_person(State(k): State<Arc<Kernel>>, Path(person): Path<String>) -> Response {
    let person = person.trim().to_lowercase();
    if k.person(&person).is_none() {
        // The directory changed outside the server: read it again first.
        if let Err(e) = k.accounts.refresh_people().await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    crate::grants::answered(k.computer_for(&Owner::Person(person)).await.map(|id| json!({ "id": id })))
}

/// From `magnus person add`: asks a running server to make the person's
/// computer now. `Ok(false)` when no server is listening.
pub(crate) async fn tell_server(state_dir: &std::path::Path, person: &str) -> anyhow::Result<bool> {
    let Ok(mut s) = tokio::net::UnixStream::connect(crate::control::socket_path(state_dir)).await else { return Ok(false) };
    let req = format!("POST /v1/people/{person}/computer HTTP/1.1\r\nHost: magnus\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let answer = tokio::time::timeout(Duration::from_secs(TELL_WAIT_SECONDS), async {
        s.write_all(req.as_bytes()).await?;
        let mut out = String::new();
        s.read_to_string(&mut out).await?;
        anyhow::Ok(out)
    })
    .await??;
    let status = answer.split_whitespace().nth(1).unwrap_or_default();
    anyhow::ensure!(status == "200", "the server did not make the computer: {}", answer.lines().last().unwrap_or_default());
    Ok(true)
}
