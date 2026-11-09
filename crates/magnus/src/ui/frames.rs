//! Pages that run scripts. A frame made from `srcdoc` takes the app's own
//! content security policy, which forbids them, so each page is served once
//! under its own: no network and a sandbox even if opened directly.
use super::docs::HTML_CSP;
use crate::api::{err, who};
use crate::kernel::{new_id, Kernel};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_WAITING_FRAMES: usize = 256;
const FRAME_LIFETIME: Duration = Duration::from_secs(60);

static WAITING: Mutex<Option<HashMap<String, (Instant, String)>>> = Mutex::new(None);

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/ui/frames", post(make)).route("/v1/ui/frames/{id}", get(serve))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    html: String,
}

fn hold(html: String) -> Option<String> {
    let mut slot = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    let waiting = slot.get_or_insert_with(HashMap::new);
    waiting.retain(|_, (at, _)| at.elapsed() < FRAME_LIFETIME);
    if waiting.len() >= MAX_WAITING_FRAMES {
        return None;
    }
    let id = new_id("frame");
    waiting.insert(id.clone(), (Instant::now(), html));
    Some(id)
}

fn claim(id: &str) -> Option<String> {
    let mut slot = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_mut()?.remove(id).filter(|(at, _)| at.elapsed() < FRAME_LIFETIME).map(|(_, html)| html)
}

async fn make(State(k): State<Arc<Kernel>>, h: HeaderMap, Json(b): Json<Frame>) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    if b.html.len() > MAX_FRAME_BYTES {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "that page is too large");
    }
    match hold(b.html) {
        Some(id) => Json(json!({ "url": format!("/v1/ui/frames/{id}") })).into_response(),
        None => err(StatusCode::TOO_MANY_REQUESTS, "too many pages are waiting"),
    }
}

/// Needs no sign-in, since the frame has no origin to send one from. The
/// id is long, works once and soon stops working.
async fn serve(Path(id): Path<String>) -> Response {
    let Some(html) = claim(&id) else { return err(StatusCode::NOT_FOUND, "no such page") };
    let policy = format!("{HTML_CSP}; sandbox allow-scripts");
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()), (header::CONTENT_SECURITY_POLICY, policy), (header::CACHE_CONTROL, "no-store".to_string())], html)
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_served_once() {
        let id = hold("<p>".to_string()).unwrap();
        assert_eq!(claim(&id).as_deref(), Some("<p>"));
        assert_eq!(claim(&id), None);
        assert_eq!(claim("frame_unknown"), None);
    }
}
