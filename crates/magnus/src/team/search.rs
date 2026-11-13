//! One search over a workspace: messages, files, tasks and knowledge, each
//! limited to what the person searching may see.
use super::member_view::{MemberView, Runs};
use super::routes::{member, Reply, K};
use super::Role;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_RESULTS_PER_KIND: usize = 20;
const SNIPPET_CHARS: usize = 160;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/search", get(search))
}

fn snippet(text: &str) -> String {
    text.chars().take(SNIPPET_CHARS).collect()
}

/// What matches every word of the query, by kind.
pub fn find<R: Runs>(v: &MemberView<R>, query: &str) -> Value {
    let (w, person) = (v.workspace(), v.person());
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return json!({ "messages": [], "files": [], "tasks": [], "knowledge": [] });
    }
    let hit = |text: &str| {
        let t = text.to_lowercase();
        words.iter().all(|w| t.contains(w.as_str()))
    };
    let mut messages = vec![];
    for (id, ch) in v.channels() {
        for m in v.messages(id).into_iter().rev().filter(|m| hit(&m.text)).take(MAX_RESULTS_PER_KIND) {
            messages.push(json!({ "channel": id, "channel_name": ch.name, "id": m.id, "thread": m.thread, "sender": m.sender, "text": snippet(&m.text), "at": m.at }));
        }
    }
    messages.sort_by_key(|m| std::cmp::Reverse(super::chat_routes::when(m)));
    messages.truncate(MAX_RESULTS_PER_KIND);
    let files: Vec<Value> =
        v.files().filter(|f| hit(&f.name)).take(MAX_RESULTS_PER_KIND).map(|f| json!({ "name": f.name, "by": f.by, "project": w.chat.file_projects.get(&f.name) })).collect();
    let tasks: Vec<Value> = w
        .chat
        .tasks
        .values()
        .filter(|t| w.in_project(t.project.as_deref(), person) && hit(&t.title))
        .take(MAX_RESULTS_PER_KIND)
        .map(|t| json!({ "id": t.id, "title": t.title, "status": t.status, "project": t.project }))
        .collect();
    let knowledge: Vec<Value> = w
        .knowledge
        .values()
        .filter(|e| e.retired.is_none() && w.may_read(e.project.as_deref(), person) && hit(&e.statement))
        .take(MAX_RESULTS_PER_KIND)
        .map(|e| json!({ "id": e.id, "statement": snippet(&e.statement), "kind": e.kind, "project": e.project }))
        .collect();
    json!({ "messages": messages, "files": files, "tasks": tasks, "knowledge": knowledge })
}

async fn search(State(k): K, h: HeaderMap, Path(ws): Path<String>, Query(q): Query<HashMap<String, String>>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let query = q.get("q").cloned().unwrap_or_default();
    let out = k.read(|s| MemberView::of(s, &ws, &p.id).map(|v| find(&v, &query)));
    Json(out.unwrap_or_else(|| json!({}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::super::chat::{Direct, Task};
    use super::super::knowledge::{Entry, Project};
    use super::super::member_view::Outcome;
    use super::super::{Channel, File, Ledger, Message, Workspace};
    use super::*;
    use std::collections::BTreeMap;

    fn look(w: &Workspace, person: &str, q: &str) -> Value {
        let (ledger, runs) = (Ledger::default(), BTreeMap::<String, Outcome>::new());
        find(&MemberView::new(&ledger, w, person, &runs), q)
    }

    fn said(id: &str, text: &str) -> Message {
        Message { id: id.into(), sender: "ann".into(), text: text.into(), thread: None, at: chrono::Utc::now() }
    }

    #[test]
    fn one_search_finds_messages_files_tasks_and_knowledge_that_one_may_see() {
        let mut w = Workspace::default();
        w.members.insert("ann".into(), Role::Member);
        w.members.insert("bo".into(), Role::Member);
        w.channels.insert("c".into(), Channel { name: "general".into(), messages: vec![said("1", "The launch date moved")], project: None });
        w.channels.insert("d".into(), Channel { name: String::new(), messages: vec![said("2", "launch secret")], project: None });
        w.chat.direct.insert("d".into(), Direct { people: vec!["ann".into()], agent: false });
        w.files.insert("launch-plan.pdf".into(), File { name: "launch-plan.pdf".into(), size: 1, by: "ann".into() });
        w.projects.insert("p".into(), Project { id: "p".into(), owner: "ann".into(), ..Default::default() });
        w.chat.tasks.insert("t".into(), Task { id: "t".into(), project: Some("p".into()), title: "Plan the launch".into(), ..Default::default() });
        w.knowledge.insert("k".into(), Entry { id: "k".into(), statement: "Launch is in March".into(), ..Default::default() });
        let ann = look(&w, "ann", "LAUNCH");
        assert_eq!(ann["messages"].as_array().unwrap().len(), 2);
        assert_eq!(ann["files"].as_array().unwrap().len(), 1);
        assert_eq!(ann["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(ann["knowledge"].as_array().unwrap().len(), 1);
        let bo = look(&w, "bo", "launch");
        assert_eq!(bo["messages"].as_array().unwrap().len(), 1, "another's direct message stays out");
        assert!(bo["tasks"].as_array().unwrap().is_empty(), "a project bo is not in stays out");
        assert!(look(&w, "ann", "launch march")["messages"].as_array().unwrap().is_empty(), "every word must match");
    }
}
