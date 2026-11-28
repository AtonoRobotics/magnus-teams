use super::*;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::io::Read;
use tower::ServiceExt;

fn gunzip(b: &[u8]) -> Value {
    let mut s = String::new();
    flate2::read::GzDecoder::new(b).read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn app() -> Router {
    let big = json!({ "tasks": (0..200).map(|i| json!({ "task_id": format!("task_{i}"), "title": "Write the weekly report" })).collect::<Vec<_>>() });
    Router::new()
        .route("/big", get(move || async move { Json(big.clone()) }))
        .route("/small", get(|| async { Json(json!({ "ok": true })) }))
        .route("/stream", get(|| async { ([(header::CONTENT_TYPE, "text/event-stream")], "data: x\n\n".repeat(400)) }))
        .layer(axum::middleware::from_fn(gzip_json))
}

async fn fetch(path: &str, gzip: bool) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut r = Request::builder().uri(path);
    if gzip {
        r = r.header(header::ACCEPT_ENCODING, "gzip, deflate");
    }
    let res = app().oneshot(r.body(Body::empty()).unwrap()).await.unwrap();
    let (parts, body) = res.into_parts();
    (parts.status, parts.headers, axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec())
}

/// A list goes gzipped to a client that asks, and reads back the same.
#[tokio::test]
async fn a_json_list_is_gzipped_for_a_client_that_asks() {
    let (status, h, body) = fetch("/big", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h[header::CONTENT_ENCODING], "gzip");
    assert!(h.get_all(header::VARY).iter().any(|v| v == "accept-encoding"));
    let plain = fetch("/big", false).await.2;
    assert!(body.len() * 5 < plain.len(), "gzip {} of plain {}", body.len(), plain.len());
    assert_eq!(gunzip(&body), serde_json::from_slice::<Value>(&plain).unwrap());
}

/// An app from before asks for nothing and gets the answer as it was; a
/// small answer and a stream go as they are.
#[tokio::test]
async fn others_go_as_they_are() {
    let (_, h, body) = fetch("/big", false).await;
    assert!(h.get(header::CONTENT_ENCODING).is_none());
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["tasks"].as_array().unwrap().len(), 200);
    assert!(fetch("/small", true).await.1.get(header::CONTENT_ENCODING).is_none(), "a small answer is not gzipped");
    assert!(fetch("/stream", true).await.1.get(header::CONTENT_ENCODING).is_none(), "a stream is not gzipped");
}

/// Through the API itself: the person's task list.
#[tokio::test(flavor = "multi_thread")]
async fn the_task_list_comes_gzipped() {
    let s = crate::admin_app::tests::server_with("", |_| Router::new()).await;
    for i in 0..6 {
        s.k.create_run("person:mo", "mo", format!("Write the weekly report for team {i}."), vec![], crate::kernel::NewRun::default()).await.unwrap();
    }
    let token = s.token("mo").await;
    let r = reqwest::Client::new().get(format!("{}/v1/tasks", s.base)).bearer_auth(&token).header("accept-encoding", "gzip").send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-encoding"], "gzip");
    let v = gunzip(&r.bytes().await.unwrap());
    assert_eq!(v["tasks"].as_array().unwrap().len(), 6, "{v}");
    s.done().await;
}
