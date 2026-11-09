//! A button in something the agent made. It starts a new request, labelled
//! as written by the agent, and that request goes through the boundary like
//! any other. Nothing here can answer an approval: the approval surface is
//! the only route that resolves one.
use crate::api::{err, who};
use crate::kernel::{Kernel, NewRun};
use crate::tools::Label;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/tasks/{task}/actions/{index}", post(pressed)).merge(crate::local::router()).merge(crate::team::router())
}

/// The server reads the action from what the task delivered, so a client
/// cannot change what a button asks for.
async fn pressed(State(k): State<Arc<Kernel>>, h: HeaderMap, Path((task, index)): Path<(String, usize)>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let component = k.read(|s| {
        let t = s.tasks.get(&task).filter(|t| t.person == p.id)?;
        if let Some(c) = s.runs.get(&t.run_id).and_then(|r| r.outcome_view.as_ref()).and_then(|v| v.get(index)) {
            return Some(c.clone());
        }
        // A deliverable waiting on the person's review is on its card.
        s.items.values().find(|i| i.task.as_deref() == Some(&task) && i.resolution.is_none()).and_then(|i| i.details["view"].get(index).cloned())
    });
    let Some(c) = component.filter(|c| c["type"] == "action") else { return err(StatusCode::NOT_FOUND, "no such action") };
    let text = format!(
        "The person pressed \"{}\" on something you made earlier. It asks for: {}\n\nThat button is a request for this work and approves nothing.",
        c["label"].as_str().unwrap_or_default(),
        c["request"].as_str().unwrap_or_default()
    );
    let new_run = NewRun { label: Some(Label::UNTRUSTED), ..Default::default() };
    match k.create_run(&format!("person:{}", p.id), &p.id, text, vec![], new_run).await {
        Ok((task, run)) => {
            k.wake(&run);
            Json(json!({ "task_id": task, "run_id": run })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}
