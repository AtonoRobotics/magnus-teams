//! Sharing a personal project in one step: a shared project with its name,
//! instructions and files, and the people the person picked as members
//! (2026-11-28-projects-without-hoops). Before, the app did it in many calls
//! behind a dialog of choices. The personal project's conversations stay in
//! the person's own store, which a shared project never reads
//! (2026-11-13-one-chat-store-not-yet).

use super::agent::volume_dir;
use super::routes::{membership, send, FIRST_CHANNEL};
use super::{fit_name, Change, File, Role};
use crate::api::who;
use crate::api_error::{refuse, ApiResult};
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/projects/{id}/share", post(share))
}

#[derive(Deserialize)]
struct Share {
    #[serde(default)]
    people: Vec<String>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

async fn share(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Share>) -> ApiResult {
    let p = who(&k, &h).await?;
    if crate::people::rights(&p).held_to_guest_limits() {
        return refuse(StatusCode::FORBIDDEN, "guests cannot create a shared project");
    }
    let Some(project) = k.read(|s| s.library.projects().get(&id).filter(|x| x.person == p.id).cloned()) else {
        return refuse(StatusCode::NOT_FOUND, "no such project");
    };
    if let Some(missing) = b.people.iter().find(|who| k.person(who).is_none()) {
        return refuse(StatusCode::NOT_FOUND, format!("no such person: {missing}"));
    }
    let files: Vec<crate::state::LibFile> =
        k.read(|s| s.library.files().values().filter(|f| f.person == p.id && s.project_of(&f.id).as_deref() == Some(id.as_str())).cloned().collect());

    let ws = new_id();
    send(&k, &p, Change::Created { workspace: ws.clone(), name: project.name.clone(), owner: p.id.clone() }).await?;
    send(&k, &p, Change::Channel { workspace: ws.clone(), channel: new_id(), name: FIRST_CHANNEL.into() }).await?;
    if let Some(text) = project.instructions.clone().filter(|t| !t.trim().is_empty()) {
        send(&k, &p, Change::Instructions { workspace: ws.clone(), text: Some(text), by: p.id.clone() }).await?;
    }
    let dir = volume_dir(&k, &ws);
    for f in files.iter().filter(|f| fit_name(&f.name)) {
        let bytes = match k.file_bytes(f).await {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        };
        if let Err(e) = async {
            tokio::fs::create_dir_all(&dir).await?;
            tokio::fs::write(dir.join(&f.name), &bytes).await
        }
        .await
        {
            return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
        }
        let file = File { name: f.name.clone(), size: bytes.len() as u64, by: p.id.clone(), coedit: false };
        send(&k, &p, Change::File { workspace: ws.clone(), file }).await?;
    }
    for person in b.people.iter().filter(|who| **who != p.id) {
        membership(&k, &p, Role::Owner, &ws, person, Some(Role::Member)).await?;
    }
    Ok(Json(json!({ "workspace": ws })).into_response())
}

type K = State<Arc<Kernel>>;
