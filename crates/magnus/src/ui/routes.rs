//! The catalog over HTTP, and the two tools the agent uses on it.
use super::{catalog, deck, docs, spec};
use crate::api::{err, who};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::run::Flow;
use crate::state::Run;
use crate::tools::Label;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/ui/catalog", get(served).put(advertised))
        .route("/v1/ui/docs/{id}/export/{format}", get(exported))
        .merge(super::images::router())
        .merge(super::apps::router())
}

async fn served(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(_) => Json(catalog::as_json()).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Advertised {
    version: u32,
    elements: BTreeSet<String>,
}

/// The client says what it can draw. An older client is told so.
async fn advertised(State(k): State<Arc<Kernel>>, h: HeaderMap, Json(b): Json<Advertised>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if b.version != catalog::CATALOG_VERSION {
        return err(StatusCode::CONFLICT, format!("this appliance uses catalog version {}", catalog::CATALOG_VERSION));
    }
    catalog::advertise(&p.id, b.elements);
    Json(json!({ "elements": catalog::allowed(&p.id) })).into_response()
}

/// A deck the person's work wrote, as a file.
async fn exported(State(k): State<Arc<Kernel>>, h: HeaderMap, Path((id, format)): Path<(String, String)>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let compiled = k.read(|s| docs::documents(s, &p.id).remove(&id)).and_then(|t| spec::compile(&t, &catalog::allowed(&p.id)).ok());
    let Some(deck) = compiled.and_then(|c| c.into_iter().find(|e| e["type"] == "deck")) else { return err(StatusCode::NOT_FOUND, "no such deck") };
    let (bytes, media) = match format.as_str() {
        "pptx" => (deck::pptx(&deck), "application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "pdf" => (deck::pdf(&deck), "application/pdf"),
        _ => return err(StatusCode::NOT_FOUND, "decks export as pptx or pdf"),
    };
    ([(header::CONTENT_TYPE, media.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.{format}\""))], bytes).into_response()
}

impl Kernel {
    /// The loop's handling of the two catalog tools, if it is one of them.
    pub(crate) async fn ui_record(&self, run: &Run, call_id: &str, name: &str, input: &Value) -> Option<Result<Flow>> {
        match name {
            "ui_catalog" => Some(self.ui_catalog(run, call_id, input).await),
            docs::EDIT_TOOL => Some(self.ui_edit(run, call_id, input).await),
            docs::SCREENSHOT_TOOL => Some(self.html_screenshot(run, call_id, input).await),
            _ => None,
        }
    }

    /// The agent checks its own HTML: drawn offline under the same policy.
    pub(crate) async fn html_screenshot(&self, run: &Run, call_id: &str, input: &Value) -> Result<Flow> {
        let id = input["id"].as_str().unwrap_or_default();
        let Some(body) = self.read(|s| docs::documents(s, &run.person).remove(id)) else {
            self.result(&run.id, call_id, format!("There is no block {id:?}."), true).await?;
            return Ok(Flow::Continue);
        };
        let html = format!("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"{}\">{body}", docs::HTML_CSP);
        match self.render_html(&run.id, &run.person, &html).await {
            Ok(image) => {
                let ev = Ev::ToolResult {
                    run: run.id.clone(),
                    call_id: call_id.to_string(),
                    key: None,
                    status: "ok".into(),
                    text: format!("Screenshot of {id} attached."),
                    is_error: false,
                    label: Label::PRINCIPAL,
                    data: Some(json!({ "image_b64": image })),
                    sources: vec![],
                    check: false,
                };
                self.emit("kernel", ev).await?;
            }
            Err(e) => self.result(&run.id, call_id, format!("It could not be drawn: {e:#}"), true).await?,
        }
        Ok(Flow::Continue)
    }

    /// The catalog's docs, loaded when wanted so the prompt stays the same.
    pub(crate) async fn ui_catalog(&self, run: &Run, call_id: &str, input: &Value) -> Result<Flow> {
        self.result(&run.id, call_id, catalog::described(&run.person, input["element"].as_str()), false).await?;
        Ok(Flow::Continue)
    }

    /// A search and replace on a block the person's work has written. What
    /// it makes must still draw, and clients get the result on the stream.
    pub(crate) async fn ui_edit(&self, run: &Run, call_id: &str, input: &Value) -> Result<Flow> {
        let (id, find, replace) = (input["id"].as_str().unwrap_or_default(), input["find"].as_str().unwrap_or_default(), input["replace"].as_str().unwrap_or_default());
        let refused = |why: String| format!("Nothing was changed: {why}.");
        let Some(text) = self.read(|s| docs::documents(s, &run.person).remove(id)) else {
            self.result(&run.id, call_id, refused(format!("there is no block {id:?}")), true).await?;
            return Ok(Flow::Continue);
        };
        let html = text.trim_start().starts_with('<');
        let new = docs::edited(&text, find, replace).and_then(|new| {
            let fence = docs::Fence { html, id: Some(id.to_string()), body: new.clone(), range: 0..0 };
            docs::problem(&fence, &run.person, &[]).map_or(Ok(new), Err)
        });
        match new {
            Ok(new) => {
                let data = json!({ "id": id, "text": new, "patch": { "find": find, "replace": replace } });
                let ev = Ev::ToolResult {
                    run: run.id.clone(),
                    call_id: call_id.to_string(),
                    key: None,
                    status: "ok".into(),
                    text: format!("Edited {id}."),
                    is_error: false,
                    label: Label::PRINCIPAL,
                    data: Some(data),
                    sources: vec![],
                    check: false,
                };
                self.emit("kernel", ev).await?;
            }
            Err(why) => self.result(&run.id, call_id, refused(why), true).await?,
        }
        Ok(Flow::Continue)
    }
}
