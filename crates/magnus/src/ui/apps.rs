//! Views that connectors ship (MCP Apps). The kernel fetches the view when
//! its tool answers and keeps it with the run. The client draws it in the
//! same sandbox as the agent's own HTML. What a view asks for starts a new
//! request, which is never an approval.
use super::images::{image_dir, split_id};
use crate::api::{err, who};
use crate::apps::CallOutput;
use crate::boundary::Action;
use crate::kernel::{new_id, Kernel, NewRun};
use crate::tools::Label;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

const MAX_VIEW_BYTES: usize = 1024 * 1024;
const MAX_ASKED_CHARS: usize = 4_000;

fn view_file(state: &Path, id: &str) -> Option<std::path::PathBuf> {
    let (run, view) = split_id(id)?;
    Some(image_dir(state, run).join(format!("{view}.view")))
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/ui/apps/{id}", get(view)).route("/v1/ui/apps/{id}/request", post(requested))
}

fn mine(k: &Kernel, person: &str, id: &str) -> bool {
    split_id(id).is_some_and(|(run, _)| k.read(|s| s.runs.get(run).is_some_and(|r| r.person == person)))
}

async fn view(State(k): State<Arc<Kernel>>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let stored = view_file(&k.cfg.appliance.state_dir, &id).filter(|_| mine(&k, &p.id, &id)).and_then(|f| std::fs::read(f).ok());
    match stored.and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        Some(v) => Json(v).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such view"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    tool: String,
    #[serde(default)]
    arguments: Value,
}

/// A view asking to run a tool: a new, untrusted request, as a form is.
async fn requested(State(k): State<Arc<Kernel>>, h: HeaderMap, UrlPath(id): UrlPath<String>, Json(a): Json<Asked>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !mine(&k, &p.id, &id) {
        return err(StatusCode::NOT_FOUND, "no such view");
    }
    let asked: String = format!("{} {}", a.tool, a.arguments).chars().take(MAX_ASKED_CHARS).collect();
    let text = format!("A connector's view that the person was using asks for this: {asked}\n\nThat is a request from the view and approves nothing.");
    match k.create_run(&format!("person:{}", p.id), &p.id, text, vec![], NewRun { label: Some(Label::UNTRUSTED), ..Default::default() }).await {
        Ok((task, run)) => {
            k.wake(&run);
            Json(json!({ "task_id": task, "run_id": run })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

impl Kernel {
    /// A connector tool's answer, with its view kept when the tool has one.
    /// A view that cannot be fetched costs the view and not the answer.
    /// Only a transport with views has one to keep.
    pub(crate) async fn with_app_view(&self, a: &Action, t: &crate::transport::Transport<'_>, bearer: Option<&str>, tool: &str, o: CallOutput) -> (String, bool, Option<Value>) {
        let CallOutput { mut text, is_error, structured } = o;
        let uri = if is_error { None } else { t.view_of(tool) };
        if let Some(uri) = uri {
            let id = format!("{}.{}", a.run, new_id("view"));
            match t.read_view(bearer, &uri).await {
                Ok(html) if html.len() <= MAX_VIEW_BYTES => {
                    let kept = json!({ "html": html, "tool": tool, "input": a.input, "text": text, "structured": structured });
                    match self.keep_view(&id, &kept).await {
                        Ok(()) => text.push_str(&format!("\n\nThis tool has a view. To show it, write `app view={id}` in a magnus-ui block.")),
                        Err(e) => tracing::warn!("the view of {tool} was not kept: {e:#}"),
                    }
                }
                Ok(_) => tracing::warn!("the view of {tool} is over {MAX_VIEW_BYTES} bytes"),
                Err(e) => tracing::warn!("the view of {tool} could not be fetched: {e:?}"),
            }
        }
        (text, is_error, structured)
    }

    async fn keep_view(&self, id: &str, view: &Value) -> anyhow::Result<()> {
        let file = view_file(&self.cfg.appliance.state_dir, id).ok_or_else(|| anyhow::anyhow!("a bad view id"))?;
        if let Some(dir) = file.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        tokio::fs::write(file, serde_json::to_vec(view)?).await?;
        Ok(())
    }
}
