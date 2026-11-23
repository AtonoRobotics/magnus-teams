//! One request shape for saying something to Magnus, wherever it is said: a
//! message in the person's own chat, a message in a shared conversation, and
//! a whisper to their own Magnus there. Each carries its text, the person's
//! own files and the model as the composer picked it, reasoning included,
//! and each is checked the same way, so none drops a part the others keep
//! (decisions/2026-11-23-one-request-shape-for-shared-conversations.md).
use crate::api_error::{refuse, ApiResult};
use crate::config::Person;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default, Debug)]
pub struct Say {
    /// What was said. Each place has its own rule for how much.
    #[serde(default)]
    pub text: String,
    /// The person's own uploaded files, by their library ids.
    #[serde(default)]
    pub files: Vec<String>,
    /// `{"adapter", "model", "reasoning"}` as the picker chose it; the
    /// server's default when absent.
    #[serde(default)]
    pub model: Value,
}

/// The model and the reasoning effort a request picked, each checked.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Pick {
    pub model: Option<(String, String)>,
    pub reasoning: Option<String>,
}

/// A `Say`, checked: its files are the person's own uploads, its model one
/// an adapter offers, its reasoning one of the efforts.
#[derive(Default, Debug)]
pub struct Said {
    pub text: String,
    pub files: Vec<String>,
    pub pick: Pick,
}

impl Say {
    /// The shape inside a body that also carries a place's own keys.
    pub fn of(b: &Value) -> ApiResult<Say> {
        serde_json::from_value(b.clone()).or_else(|e| refuse(StatusCode::BAD_REQUEST, format!("{e}")))
    }

    pub async fn check(self, k: &Kernel, p: &Person) -> ApiResult<Said> {
        let model = json!({ "model": self.model });
        let pick = Pick { model: crate::api::chosen_model(k, &model).await?, reasoning: crate::api::chosen_reasoning(&model)? };
        let files = crate::library::given_files(k, p, &json!({ "files": self.files }))?;
        Ok(Said { text: self.text, files, pick })
    }
}

#[cfg(test)]
#[path = "say_tests.rs"]
mod tests;
