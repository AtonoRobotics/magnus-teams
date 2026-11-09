//! A button in something the agent made. It starts a new request, labelled
//! as written by the agent, and that request goes through the boundary like
//! any other. Nothing here can answer an approval: the approval surface is
//! the only route that resolves one.
use crate::api::{err, who};
use crate::kernel::{Kernel, NewRun};
use crate::tools::Label;
use crate::ui::docs;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;

/// How much of one answer in a form is kept.
const MAX_ANSWER_CHARS: usize = 4_000;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/tasks/{task}/actions/{index}", post(pressed)).merge(crate::local::router()).merge(crate::team::router()).merge(crate::ui::routes::router())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Pressed {
    /// Which block of the answer, when it is not in the deliverable.
    #[serde(default)]
    fence: Option<usize>,
    #[serde(default)]
    values: Map<String, Value>,
}

/// The element at `index` of a spec the answer holds.
fn in_answer(k: &Kernel, person: &str, run: &str, fence: usize, index: usize) -> Option<Value> {
    k.read(|s| {
        let text = docs::current_text(s, person, &s.runs.get(run)?.summary)?;
        let block = docs::fences(&text).into_iter().nth(fence).filter(|f| !f.html)?;
        crate::ui::spec::compile(&block.body, &crate::ui::catalog::allowed(person)).ok()?.into_iter().nth(index)
    })
}

/// A form's answers, only the fields it asked for, as the request's words.
fn answers_of(c: &Value, values: &Map<String, Value>) -> String {
    let fields = c["fields"].as_array().map_or(&[][..], Vec::as_slice);
    let lines = fields.iter().filter_map(|f| {
        let name = f["name"].as_str()?;
        let value: String = values.get(name)?.as_str()?.chars().take(MAX_ANSWER_CHARS).collect();
        Some(format!("{name}: {value}"))
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// The server reads what was pressed from what the task delivered, so a
/// client cannot change what a button asks for.
async fn pressed(State(k): State<Arc<Kernel>>, h: HeaderMap, Path((task, index)): Path<(String, usize)>, body: Option<Json<Pressed>>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Pressed { fence, values } = body.map(|Json(b)| b).unwrap_or_default();
    let Some(run) = k.read(|s| s.tasks.get(&task).filter(|t| t.person == p.id).map(|t| t.run_id.clone())) else { return err(StatusCode::NOT_FOUND, "no such action") };
    let component = match fence {
        Some(n) => in_answer(&k, &p.id, &run, n, index),
        None => k.read(|s| {
            if let Some(c) = s.runs.get(&run).and_then(|r| r.outcome_view.as_ref()).and_then(|v| v.get(index)) {
                return Some(c.clone());
            }
            // A deliverable waiting on the person's review is on its card.
            s.items.values().find(|i| i.task.as_deref() == Some(&task) && i.resolution.is_none()).and_then(|i| i.details["view"].get(index).cloned())
        }),
    };
    let Some(c) = component.filter(|c| c["type"] == "action" || c["type"] == "form") else { return err(StatusCode::NOT_FOUND, "no such action") };
    let answers = answers_of(&c, &values);
    let text = format!(
        "The person pressed \"{}\" on something you made earlier. It asks for: {}\n{answers}\n\nThat button is a request for this work and approves nothing.",
        c["label"].as_str().or(c["submit"].as_str()).unwrap_or_default(),
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
