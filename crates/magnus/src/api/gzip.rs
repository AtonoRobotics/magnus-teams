//! The API's JSON answers go gzipped to a client that accepts gzip: lists
//! of tasks, threads and items compress about ten to one, and the desktop
//! app reaches the server over Tailscale, where each refresh's megabytes
//! once kept a reply from showing for half a minute (2026-11-27). A client
//! that does not ask (an app from before) gets the answer as it was.
//! Streams (events, the screen) and the UI's files, gzipped already, pass
//! through untouched.

use axum::body::{Body, HttpBody};
use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use flate2::{write::GzEncoder, Compression};
use std::io::Write;

/// Below this an answer goes as it is: gzip's own header and the work cost
/// more than they save.
pub(crate) const MIN_GZIP_BYTES: u64 = 1024;

pub(crate) async fn gzip_json(req: Request, next: Next) -> Response {
    let accepts = crate::ui::assets::accepts_gzip(req.headers());
    let res = next.run(req).await;
    if !accepts || res.headers().contains_key(header::CONTENT_ENCODING) || !is_json(&res) {
        return res;
    }
    // Only a whole answer of known size: a stream is never held back.
    match res.body().size_hint().exact() {
        Some(n) if n >= MIN_GZIP_BYTES => {}
        _ => return res,
    }
    let (mut parts, body) = res.into_parts();
    let Ok(raw) = axum::body::to_bytes(body, usize::MAX).await else {
        // The body is gone; what was read is not an answer.
        return Response::from_parts(parts, Body::empty());
    };
    let Some(gz) = gzip(&raw) else {
        return Response::from_parts(parts, Body::from(raw));
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    parts.headers.append(header::VARY, HeaderValue::from_static("accept-encoding"));
    Response::from_parts(parts, Body::from(gz))
}

fn is_json(res: &Response) -> bool {
    res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"))
}

fn gzip(raw: &[u8]) -> Option<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(raw).ok()?;
    enc.finish().ok()
}

#[cfg(test)]
#[path = "gzip_tests.rs"]
mod tests;
