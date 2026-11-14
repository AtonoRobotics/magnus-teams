//! Who is asking, resolved once, before a route runs (decision
//! `2026-11-14-handlers-take-a-caller.md`): a route names what it needs in
//! its arguments, `Caller` for anyone signed in, `Owner` for the server's
//! owner, `Developer` for the developer view, `AppSession` for the app's own
//! session, and `Body<T>` for a request body of a fixed shape. Each refuses
//! as the hand-written check before it did, with the same status and body.

use crate::api::{credential, err, who};
use crate::api_error::ApiError;
use crate::config::Person;
use crate::kernel::Kernel;
use crate::people::rights;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::de::DeserializeOwned;
use std::sync::Arc;

/// Whoever is signed in: a 401 `sign in` without a credential, `sign in
/// again` with one that holds no longer.
pub struct Caller(pub Person);

/// The server's owner: a 403 `this needs the owner role` for anyone else.
pub struct Owner(pub Person);

/// Someone with the developer view: a 403 `this needs the developer role`
/// for anyone else.
pub struct Developer(pub Person);

/// Someone on the app's own session, a bearer token the app signed in for,
/// never a browser's cookie: a 403 for any other.
pub struct AppSession(pub Person);

/// A request body of a fixed shape. One that does not parse, or is not
/// JSON, is refused as every refusal is, `{"error": ...}` with its status,
/// instead of being read as empty.
pub struct Body<T>(pub T);

impl FromRequestParts<Arc<Kernel>> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, k: &Arc<Kernel>) -> Result<Caller, ApiError> {
        Ok(Caller(who(k, &parts.headers).await?))
    }
}

impl FromRequestParts<Arc<Kernel>> for Owner {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, k: &Arc<Kernel>) -> Result<Owner, ApiError> {
        Ok(Owner(owner(k, &parts.headers).await?))
    }
}

impl FromRequestParts<Arc<Kernel>> for Developer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, k: &Arc<Kernel>) -> Result<Developer, ApiError> {
        Ok(Developer(developer(k, &parts.headers).await?))
    }
}

impl FromRequestParts<Arc<Kernel>> for AppSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, k: &Arc<Kernel>) -> Result<AppSession, ApiError> {
        Ok(AppSession(app_session(k, &parts.headers).await?))
    }
}

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Body<T>, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Body(v)),
            Err(r) => Err(ApiError::new(r.status(), r.body_text())),
        }
    }
}

/// The server's owner, for a route that asks in its own time.
pub(crate) async fn owner(k: &Kernel, h: &HeaderMap) -> Result<Person, ApiError> {
    let p = who(k, h).await?;
    if !rights(&p).is_server_owner() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "this needs the owner role"));
    }
    Ok(p)
}

/// Someone with the developer view, for a route that asks in its own time.
pub(crate) async fn developer(k: &Kernel, h: &HeaderMap) -> Result<Person, ApiError> {
    let p = who(k, h).await?;
    if !rights(&p).sees_developer_view() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "this needs the developer role"));
    }
    Ok(p)
}

/// Someone on the app's own session, for a route that asks in its own time.
pub(crate) async fn app_session(k: &Kernel, h: &HeaderMap) -> Result<Person, ApiError> {
    let p = who(k, h).await?;
    let token = credential(h).unwrap_or_default();
    let bearer = h.get("authorization").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Bearer "));
    match k.accounts.session(token).await {
        Some(s) if bearer && s.client == "app" => Ok(p),
        _ => Err(err(StatusCode::FORBIDDEN, "only the Magnus app on that computer does this").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::Router;
    use serde::Deserialize;
    use tower::ServiceExt;

    #[derive(Deserialize)]
    struct Secret {
        client_secret: String,
    }

    async fn send(body: &'static str, content_type: &str) -> (u16, serde_json::Value) {
        let app = Router::new().route("/", post(|Body(b): Body<Secret>| async move { b.client_secret }));
        let req = axum::http::Request::post("/").header("content-type", content_type).body(axum::body::Body::from(body)).unwrap();
        let r = app.oneshot(req).await.unwrap();
        let status = r.status().as_u16();
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into())))
    }

    /// A body without its field is refused, not read as an empty secret, and
    /// every refusal of a body is `{"error": ...}`.
    #[tokio::test]
    async fn a_bad_body_is_refused_as_json() {
        assert_eq!(send(r#"{"client_secret":"s"}"#, "application/json").await, (200, serde_json::Value::String("s".into())));
        for (body, content_type, status) in
            [("{}", "application/json", 422), (r#"{"client_secret":7}"#, "application/json", 422), ("{", "application/json", 400), ("{}", "text/plain", 415)]
        {
            let (got, v) = send(body, content_type).await;
            assert_eq!(got, status, "{body} as {content_type}: {v}");
            assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{body} as {content_type}: {v}");
        }
    }
}
