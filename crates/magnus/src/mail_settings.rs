//! The owner's view of the server's outgoing mail in Settings: what `[mail]`
//! says, whether its password is in the vault, and a test email. The
//! password itself goes in through the vault route (`PUT /v1/secrets/{name}`),
//! so it never passes through chat, the config or a log.

use crate::api::{err, owner};
use crate::kernel::Kernel;
use crate::mail_config::Mail;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/mail", get(status)).route("/v1/mail/test", post(test))
}

/// What Settings shows: nothing secret, only whether the secret is held.
fn describe(mail: Option<&Mail>, password_set: bool, address: Option<String>) -> Value {
    let Some(m) = mail else { return json!({ "configured": false, "address": address }) };
    json!({
        "configured": true,
        "host": m.host,
        "port": m.port,
        "tls": m.tls,
        "from": m.from,
        "username": m.username,
        "credential": m.credential,
        "password_set": password_set,
        "address": address,
    })
}

async fn status(State(k): K, h: HeaderMap) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let mail = k.cfg.mail.as_ref();
    let password_set = mail.and_then(|m| m.credential.as_ref()).is_some_and(|c| matches!(k.vault.get(c), Ok(Some(_))));
    let address = k.accounts.local_person(&p.id).and_then(|l| l.email);
    Json(describe(mail, password_set, address)).into_response()
}

/// Send a test email now and say how it went, so the owner learns of a
/// wrong password or a blocked port here and not from a lost recovery.
async fn test(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let to = b["to"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).or_else(|| k.accounts.local_person(&p.id).and_then(|l| l.email));
    let Some(to) = to else { return err(StatusCode::BAD_REQUEST, "Say which address to send the test to.") };
    let text = "This is a test from your Magnus server. Password recovery emails will come from this address.".to_string();
    match crate::mailer::send(&k, &to, "Magnus test email", text).await {
        Ok(true) => Json(json!({ "ok": true, "to": to })).into_response(),
        Ok(false) => err(StatusCode::CONFLICT, "The server has no outgoing mail set up."),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("The test email was not sent: {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::describe;
    use crate::mail_config::Mail;

    /// Settings learns the setup and whether a password is held, and never
    /// the password; with no `[mail]` it learns only that.
    #[test]
    fn settings_sees_the_setup_and_never_a_secret() {
        let m = Mail {
            host: "smtp.example.com".into(),
            port: 465,
            tls: "tls".into(),
            username: Some("magnus@example.com".into()),
            credential: Some("smtp_password".into()),
            from: "magnus@example.com".into(),
            link_base: None,
        };
        let v = describe(Some(&m), true, Some("owner@example.com".into()));
        assert_eq!(v["configured"], true);
        assert_eq!(v["password_set"], true);
        assert_eq!(v["credential"], "smtp_password");
        assert_eq!(v["address"], "owner@example.com");
        assert!(v.get("password").is_none());
        let none = describe(None, false, None);
        assert_eq!(none["configured"], false);
        assert!(none.get("host").is_none());
    }
}
