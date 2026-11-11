//! Account calls made before anyone is signed in: asking for a password
//! recovery email. They go over the server's trusted connection, like
//! signing in, and carry no session.
use crate::{body_of, message, pin, App, Res};
use serde_json::{json, Value};
use tauri::State;

/// Ask the server to email a recovery link to whoever holds `email`. The
/// server answers the same whether or not anyone does.
#[tauri::command]
pub async fn forgot_password(app: State<'_, App>, email: String) -> Res<Value> {
    let (url, c) = app.conn()?;
    let r = c
        .post(format!("{url}/v1/password-reset/request"))
        .json(&json!({ "email": email }))
        .send()
        .await
        .map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    if status >= 400 {
        return Err(message(&body, status));
    }
    Ok(body)
}
