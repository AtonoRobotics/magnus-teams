//! What a member does with a team message, on the server: share it with
//! another member, save it to the workspace's files, and ask the agent again.
//! Each finds the message through what the member sees (`MemberView`), so
//! one they do not see is not there (404), and keeps the workspace's rules
//! itself rather than trusting the app to hide a button (403):
//! - an answer holding data only its requester may see is neither shared
//!   nor saved for the team (its requester posts it to the thread instead);
//! - a guest saves nothing and has the agent do nothing;
//! - only who asked asks again.
//!
//! Branch stays in the app: it opens a new private chat with the message
//! quoted in the composer, unsent, so the team's words reach the person's
//! own agent only when they send them (decisions/2026-11-13-team-message-actions-on-the-server.md).
use super::agent::volume_dir;
use super::chat::Chat;
use super::chat_routes::{direct_between, name_of};
use super::member_view::{MemberView, ANSWER_PREFIX};
use super::routes::{member, new_id, post_and_ask, send, Reply, K};
use super::{Change, File, Message, Role, Workspace};
use crate::api::err;
use crate::kernel::Kernel;
use crate::message_file;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// How many numbered names a saved message tries before giving up.
const MAX_NAME_TRIES: usize = 100;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/share", post(share))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/save", post(save))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/retry", post(retry))
}

/// A message as the member sees it, with what the routes here need.
struct Found {
    author: String,
    requester: Option<String>,
    text: String,
    at: chrono::DateTime<chrono::Utc>,
    private: bool,
    task: Option<String>,
    /// The channel's project, if it is in one.
    project: Option<String>,
    workspace: Workspace,
}

/// The message with words that this person sees, or a 404 that does not say
/// whether it exists.
fn found(k: &Kernel, ws: &str, channel: &str, id: &str, person: &str) -> Result<Found, Box<Reply>> {
    let now = chrono::Utc::now();
    let f = k.read(|s| {
        let v = MemberView::of(s, ws, person)?;
        let m = v.message(channel, id, now).filter(|m| !m.text().is_empty())?;
        Some(Found {
            author: m.author().to_string(),
            requester: m.requester().map(String::from),
            text: m.text().to_string(),
            at: m.at(),
            private: m.private(),
            task: m.task().map(String::from),
            project: v.workspace().channels.get(channel).and_then(|c| c.project.clone()),
            workspace: v.workspace().clone(),
        })
    });
    f.ok_or_else(|| Box::new(err(StatusCode::NOT_FOUND, "no such message")))
}

/// What only its requester may see stays with them.
fn not_private(m: &Found) -> Result<(), Box<Reply>> {
    if m.private {
        return Err(Box::new(err(StatusCode::FORBIDDEN, "this answer holds data only its requester may see: they can post it to the thread themselves")));
    }
    Ok(())
}

/// A message quoted, as a member passes it on.
fn quoted(who: &str, text: &str) -> String {
    let lines: Vec<String> = text.lines().map(|l| format!("> {l}")).collect();
    format!("{who} wrote:\n{}", lines.join("\n"))
}

/// The first of `name`, `name-2`, `name-3` … that is not on the volume, so a
/// saved message never writes over a file.
fn free_name(w: &Workspace, name: &str) -> Option<String> {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    std::iter::once(name.to_string()).chain((2..=MAX_NAME_TRIES).map(|n| format!("{stem}-{n}.md"))).find(|n| !w.files.contains_key(n) && super::fit_name(n))
}

#[derive(Deserialize)]
struct ShareTo {
    person: String,
}

/// Send the message, quoted, to another member in a direct conversation.
/// One from a project's channel goes only to the project's people.
async fn share(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<ShareTo>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let m = ok_or_return!(found(&k, &ws, &channel, &id, &p.id));
    ok_or_return!(not_private(&m));
    let w = &m.workspace;
    if b.person == p.id || !w.members.contains_key(&b.person) {
        return err(StatusCode::NOT_FOUND, "no such member");
    }
    if !w.in_project(m.project.as_deref(), &b.person) {
        return err(StatusCode::FORBIDDEN, format!("{} is not in this channel's project", name_of(&k, &b.person)));
    }
    let mut people = vec![p.id.clone(), b.person.clone()];
    people.sort();
    let dm = ok_or_return!(direct_between(&k, &p, w, people, false).await);
    let message = Message { id: new_id(), sender: p.id.clone(), text: quoted(&name_of(&k, &m.author), &m.text), thread: None, at: chrono::Utc::now() };
    let posted = message.id.clone();
    ok_or_return!(send(&k, &p, Change::Message { workspace: ws, channel: dm.clone(), message }).await);
    (StatusCode::CREATED, Json(json!({ "channel": dm, "id": posted }))).into_response()
}

/// Save the message as a Markdown file in the workspace's files, which the
/// whole team (or the channel's project) sees.
async fn save(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let m = ok_or_return!(found(&k, &ws, &channel, &id, &p.id));
    ok_or_return!(not_private(&m));
    let Some(name) = free_name(&m.workspace, &message_file::file_name(&m.text)) else { return err(StatusCode::CONFLICT, "too many files have that name") };
    let body = message_file::markdown(&name_of(&k, &m.author), &message_file::at_text(m.at), &m.text);
    let dir = volume_dir(&k, &ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(&name), body.as_bytes()).await
    }
    .await
    {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    let file = File { name: name.clone(), size: body.len() as u64, by: p.id.clone() };
    ok_or_return!(send(&k, &p, Change::File { workspace: ws.clone(), file }).await);
    if m.project.is_some() {
        ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::FileIn { name: name.clone(), project: m.project } }).await);
    }
    (StatusCode::CREATED, Json(json!({ "name": name }))).into_response()
}

/// Ask the agent the same request again, where it was asked: who asked
/// posts their words again, and the agent takes them on.
async fn retry(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let m = ok_or_return!(found(&k, &ws, &channel, &id, &p.id));
    let Some(task) = m.task.as_deref().filter(|_| id.starts_with(ANSWER_PREFIX)) else { return err(StatusCode::BAD_REQUEST, "only the agent's answer is asked again") };
    if m.requester.as_deref() != Some(p.id.as_str()) {
        return err(StatusCode::FORBIDDEN, "only who asked asks again");
    }
    let asked = m.workspace.chat.asked.get(task).and_then(|a| m.workspace.channels.get(&channel)?.messages.iter().find(|x| &x.id == a && !x.text.is_empty()).cloned());
    let Some(asked) = asked.filter(|a| !m.workspace.chat.unsent.contains(&a.id)) else { return err(StatusCode::CONFLICT, "the request is no longer here to ask again") };
    post_and_ask(&k, &p, role, &ws, &channel, asked.thread, asked.text, true, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_marks_every_line() {
        assert_eq!(quoted("Mo", "one\ntwo"), "Mo wrote:\n> one\n> two");
    }

    #[test]
    fn a_saved_message_never_writes_over_a_file() {
        let mut w = Workspace::default();
        assert_eq!(free_name(&w, "plan.md").as_deref(), Some("plan.md"));
        for n in ["plan.md", "plan-2.md"] {
            w.files.insert(n.into(), File { name: n.into(), size: 1, by: "someone".into() });
        }
        assert_eq!(free_name(&w, "plan.md").as_deref(), Some("plan-3.md"));
    }
}
