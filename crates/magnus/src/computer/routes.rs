//! The computers' HTTP API.

use super::KillMode;
use crate::api::who;
use crate::grants::answered;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/computers", get(list).post(create))
        .route("/v1/computers/{id}", get(one))
        .route("/v1/computers/{id}/start", post(start))
        .route("/v1/computers/{id}/kill", post(kill))
        .route("/v1/computers/{id}/grants", post(grant))
        .route("/v1/computers/{id}/files", get(read_file).put(write_file))
}

macro_rules! person {
    ($k:expr, $h:expr) => {
        match who(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct NewComputer {
    #[serde(default)]
    workspace: Option<String>,
}

async fn create(State(k): K, h: HeaderMap, b: Option<Json<NewComputer>>) -> Response {
    let p = person!(k, h);
    let b = b.map(|Json(b)| b).unwrap_or_default();
    answered(k.create_computer(&p, b.workspace.as_deref()).await)
}

async fn list(State(k): K, h: HeaderMap) -> Response {
    let p = person!(k, h);
    answered(Ok(k.computers_of(&p)))
}

async fn one(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.computer_json(&p, &id))
}

async fn start(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.start_computer(&p, &id).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Kill {
    mode: KillMode,
    #[serde(default)]
    delete_volume: bool,
}

async fn kill(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Kill>) -> Response {
    let p = person!(k, h);
    answered(k.kill_computer(&p, &id, b.mode, b.delete_volume).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGrant {
    label: String,
}

async fn grant(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewGrant>) -> Response {
    let p = person!(k, h);
    answered(k.grant_computer(&p, &id, &b.label).await)
}

#[derive(Deserialize)]
struct FilePath {
    path: String,
}

async fn read_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = person!(k, h);
    answered(k.read_computer_file(&p, &id, &q.path).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    path: String,
    data_b64: String,
}

async fn write_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<WriteFile>) -> Response {
    let p = person!(k, h);
    answered(k.write_computer_file(&p, &id, &b.path, &b.data_b64).await)
}
