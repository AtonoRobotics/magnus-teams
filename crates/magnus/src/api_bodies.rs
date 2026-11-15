//! The bodies the server's own routes (`api`) take, each of a fixed shape,
//! so one that does not fit is refused (`extract::Body`, a 422 or 400 as
//! `{"error": ...}`) instead of read as empty (decision
//! `2026-11-14-typed-request-bodies.md`). A field a route checks itself,
//! with its own refusal, is an `Option` here, so that refusal stays the one
//! the app reads.
//!
//! Seven routes keep a `serde_json::Value`, each for its reason:
//! - `submit` and `chat_post`: `library::given_files`, `chosen_project`,
//!   `chosen_model` and `attachment::from_request` read it, the same keys for
//!   a task and a chat message;
//! - `chat_card` and `respond`: each kind of card or inbox item reads its own
//!   answer from it (`Kernel::decide_card`, `Kernel::respond`);
//! - `sign_in_complete`: `flow` and `input` are the adapter's own sign-in
//!   protocol, passed through as it gave them;
//! - `add_schedule` and `edit_schedule`: `cadence::rule_of`, `end_for` and
//!   `Kernel::edit_schedule` read it, as they read the agent's schedule tool.

use serde::Deserialize;
use serde_json::Value;

/// A device asking for its certificate: its name and its signing request.
#[derive(Deserialize)]
pub struct Enroll {
    pub name: String,
    pub csr: String,
}

/// A sign-in with a password, and a code when one is asked for.
#[derive(Deserialize)]
pub struct SignIn {
    pub person: String,
    pub password: String,
    pub code: Option<String>,
    pub machine_code: Option<String>,
    pub client: Option<String>,
}

/// A temporary password changed for one of the person's own.
#[derive(Deserialize)]
pub struct NewPassword {
    pub person: String,
    pub password: String,
    pub new_password: String,
}

/// Which client starts a sign-in: `app` for the Magnus app.
#[derive(Deserialize)]
pub struct Client {
    pub client: Option<String>,
}

/// A sign-in through the identity provider in a browser.
#[derive(Deserialize)]
pub struct BrowserBegin {
    pub client: Option<String>,
    pub redirect_uri: Option<String>,
    pub login_hint: Option<String>,
}

/// What the identity provider sent back.
#[derive(Deserialize)]
pub struct BrowserFinish {
    pub state: Option<String>,
    pub code: Option<String>,
    pub iss: Option<String>,
}

/// An invitation taken up: its token and the new password.
#[derive(Deserialize)]
pub struct AcceptInvite {
    pub token: String,
    pub password: String,
}

/// The person's verdict on an outcome, with an optional note.
#[derive(Deserialize)]
pub struct Verdict {
    pub verdict: Option<String>,
    pub note: Option<String>,
}

/// The inbox items the person has seen.
#[derive(Deserialize)]
pub struct Opened {
    #[serde(default)]
    pub ids: Vec<String>,
}

/// A policy layer's text.
#[derive(Deserialize)]
pub struct Policy {
    pub cedar: String,
}

/// A secret's value.
#[derive(Deserialize)]
pub struct Secret {
    pub value: Option<String>,
}

/// The kill switch on or off; on when the body does not say, as it always
/// was: a request to the switch that names nothing engages it.
#[derive(Deserialize)]
pub struct KillSwitch {
    pub engaged: Option<bool>,
}

/// A model adapter as the owner sets it. `command` and `settings` are the
/// adapter's own (`config::Adapter` reads them); `model` names the one its
/// conformance check runs on.
#[derive(Deserialize)]
pub struct PutAdapter {
    #[serde(default)]
    pub command: Value,
    #[serde(default = "no_settings")]
    pub settings: Value,
    pub skip_conformance: Option<bool>,
    #[serde(default)]
    pub model: Value,
}

/// The model an adapter's conformance check runs on, or its default.
#[derive(Deserialize)]
pub struct Conformance {
    #[serde(default)]
    pub model: Value,
}

/// Recorded audio to transcribe.
#[derive(Deserialize)]
pub struct Transcribe {
    pub data_b64: Option<String>,
}

/// Text to speak.
#[derive(Deserialize)]
pub struct Speak {
    pub text: Option<String>,
}

/// The server's default model.
#[derive(Deserialize)]
pub struct Model {
    pub adapter: Option<String>,
    pub model: Option<String>,
}

fn no_settings() -> Value {
    Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use crate::admin_app::tests::{server_with, PASSWORD};
    use axum::Router;
    use reqwest::Method;
    use serde_json::json;

    /// A body missing a field the route needs is refused as `{"error": ...}`,
    /// not read as empty: no sign-in with an empty password, no policy layer
    /// cleared by a body that names no text, no invitation taken with an
    /// empty token. A whole body still does what it did.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_missing_field_is_refused_not_read_as_empty() {
        let s = server_with("", |_| Router::new()).await;
        let sam = s.token("sam").await;
        for (method, path, body) in [
            (Method::POST, "/v1/sign-in", json!({ "person": "sam" })),
            (Method::POST, "/v1/password", json!({ "person": "sam", "password": PASSWORD })),
            (Method::POST, "/v1/invites/accept", json!({ "password": PASSWORD })),
            (Method::PUT, "/v1/policies/user/sam", json!({})),
            (Method::POST, "/v1/devices", json!({ "name": "Desk" })),
            (Method::POST, "/v1/inbox/opened", json!({ "ids": [7] })),
            (Method::POST, "/v1/tasks/t1/verdict", json!({ "verdict": 3 })),
        ] {
            let (st, v) = s.call(&sam, method, path, Some(body.clone())).await;
            assert_eq!(st, 422, "{path} took {body}: {v}");
            assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{path}: {v}");
        }
        let (st, v) = s.post("", "/v1/sign-in", json!({ "person": "sam", "password": PASSWORD })).await;
        assert_eq!(st, 200, "a whole sign-in was refused: {v}");
        assert_eq!(s.post(&sam, "/v1/inbox/opened", json!({ "ids": [] })).await.0, 200);
        s.done().await;
    }
}
