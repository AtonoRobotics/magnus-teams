//! One refusal for every route (decision
//! `2026-11-14-handlers-take-a-caller.md`): a handler returns
//! `ApiResult`, `?` turns a failure into the response the app already
//! reads, and `err` and `internal` keep the bodies they always had:
//! `{"error": "..."}` with its status, and for a failure the kill switch's
//! or the spend limit's own refusal, else a 500 `internal error` (the app's
//! one error policy, `desktop/ui/server.js`, reads these).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// A refusal: `{"error": msg}` with `code`.
pub(crate) fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

/// A failure: the kill switch's or the spend limit's refusal when it is
/// one of theirs, else a 500 that says no more than `internal error`, and
/// the log says the rest.
pub(crate) fn internal(e: anyhow::Error) -> Response {
    if let Some(r) = crate::halts::refused(&e) {
        return r;
    }
    if let Some(r) = crate::spend::routes::refused(&e) {
        return r;
    }
    tracing::error!("api: {e:#}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// A request refused or failed, as the response it answers with. Boxed, so
/// a result carrying it stays small.
#[derive(Debug)]
pub struct ApiError(Box<Response>);

/// What a route returns.
pub type ApiResult<T = Response> = Result<T, ApiError>;

impl ApiError {
    /// `{"error": msg}` with `code`, as `err`.
    pub fn new(code: StatusCode, msg: impl Into<String>) -> ApiError {
        ApiError(Box::new(err(code, msg)))
    }

    /// Its status.
    pub fn status(&self) -> StatusCode {
        self.0.status()
    }
}

/// Refuse with `{"error": msg}` and `code`: `return refuse(...)` in a route.
pub fn refuse<T>(code: StatusCode, msg: impl Into<String>) -> ApiResult<T> {
    Err(ApiError::new(code, msg))
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> ApiError {
        ApiError(Box::new(internal(e)))
    }
}

/// A refusal a helper already made as a response.
impl From<Response> for ApiError {
    fn from(r: Response) -> ApiError {
        ApiError(Box::new(r))
    }
}

impl From<Box<Response>> for ApiError {
    fn from(r: Box<Response>) -> ApiError {
        ApiError(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body(r: Response) -> (u16, serde_json::Value) {
        let status = r.status().as_u16();
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn a_refusal_answers_as_err_always_did() {
        let r = ApiError::new(StatusCode::NOT_FOUND, "no such task").into_response();
        assert_eq!(body(r).await, (404, json!({ "error": "no such task" })));
        let r = refuse::<()>(StatusCode::FORBIDDEN, "no").unwrap_err();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    /// A failure says no more than `internal error`; the kill switch's is
    /// its own refusal, as `internal` gives it.
    #[tokio::test]
    async fn a_failure_answers_as_internal_always_did() {
        let r = ApiError::from(anyhow::anyhow!("the database said something private")).into_response();
        assert_eq!(body(r).await, (500, json!({ "error": "internal error" })));
        let killed = || anyhow::Error::from(crate::kernel::KillSwitchOn);
        let (want, got) = (body(internal(killed())).await, body(ApiError::from(killed()).into_response()).await);
        assert_eq!(got, want);
        assert_ne!(got.0, 500, "the kill switch's refusal became a 500");
    }
}
