//! A team message's Share, Save to Files and Retry, kept on the server: a
//! message one does not see is not there, and what the workspace's rules
//! refuse is refused whatever the app shows. sam owns the workspace, mo is a
//! member, ana a guest.

use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::team::chat::Chat;
use crate::team::Change;
use axum::Router;
use serde_json::{json, Value};

/// What mo asks the agent.
const ASK: &str = "what did my inbox say";

async fn workspace(s: &Server) -> (String, String, String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    for (who, role) in [("mo", "member"), ("ana", "guest")] {
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": who, "role": role })).await.0, 200);
    }
    (ws, sam, s.token("mo").await, s.token("ana").await)
}

fn general(s: &Server, ws: &str) -> String {
    s.k.read(|st| st.team.workspace(ws).unwrap().channels.iter().find(|(_, c)| c.name == "general").map(|(id, _)| id.clone()).unwrap())
}

async fn say(s: &Server, t: &str, ws: &str, channel: &str, text: &str) -> String {
    let (st, v) = s.post(t, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": text })).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap_or_else(|| panic!("no id: {v}")).to_string()
}

/// mo asks the agent in `channel`; it answers, from data only mo may see
/// when `private`. The answer's id.
async fn answered(s: &Server, ws: &str, channel: &str, mo: &str, private: bool) -> (String, String) {
    let asked = say(s, mo, ws, channel, ASK).await;
    let (task, _) = s.k.team_request(ws, channel, None, "mo", ASK.into()).await.unwrap();
    let chat = Chat::Asked { channel: channel.into(), message: asked, task: task.clone() };
    s.k.emit("person:mo", Ev::Team(Change::Chat { workspace: ws.into(), chat })).await.unwrap();
    let run = s.k.read(|st| st.tasks[&task].run_id.clone());
    if private {
        let label = crate::tools::Label { untrusted: false, private: true };
        let read = Ev::ToolResult {
            run: run.clone(),
            call_id: "c1".into(),
            key: None,
            status: "ok".into(),
            text: String::new(),
            is_error: false,
            label,
            data: None,
            sources: vec!["mail".into()],
            check: false,
        };
        s.k.emit("kernel", read).await.unwrap();
    }
    s.k.emit("kernel", Ev::Completed { run, summary: "Your inbox says the deal closes Friday.".into(), view: None, spoken: None }).await.unwrap();
    (format!("agent:{task}"), task)
}

fn at(ws: &str, channel: &str, id: &str, act: &str) -> String {
    format!("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/{act}")
}

async fn view(s: &Server, t: &str, ws: &str, channel: &str) -> Value {
    let (st, v) = s.get(t, &format!("/v1/workspaces/{ws}/channels/{channel}/view")).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// Share sends the message, quoted, to the other member in a direct
/// conversation; nobody shares what they do not see, or with someone outside.
#[tokio::test(flavor = "multi_thread")]
async fn share_quotes_the_message_to_a_member_directly() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let c = general(&s, &ws);
    let id = say(&s, &sam, &ws, &c, "the launch moves to Friday").await;
    let (st, v) = s.post(&mo, &at(&ws, &c, &id, "share"), json!({ "person": "ana" })).await;
    assert_eq!(st, 201, "{v}");
    let dm = v["channel"].as_str().unwrap().to_string();
    let seen = view(&s, &ana, &ws, &dm).await;
    assert!(seen.to_string().contains("> the launch moves to Friday"), "{seen}");
    assert_eq!(s.post(&mo, &at(&ws, &c, &id, "share"), json!({ "person": "nobody" })).await.0, 404, "shared with someone outside");
    // A direct conversation's message is not there for someone outside it.
    let (_, d) = s.post(&sam, &format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["mo"] })).await;
    let private = say(&s, &sam, &ws, d["id"].as_str().unwrap(), "between us").await;
    let path = at(&ws, d["id"].as_str().unwrap(), &private, "share");
    assert_eq!(s.post(&ana, &path, json!({ "person": "sam" })).await.0, 404, "someone outside a conversation shared from it");
    s.done().await;
}

/// An answer holding data only its requester may see is neither shared nor
/// saved: not by others, who see only that it answered, and not by the
/// requester, who posts it to the thread instead.
#[tokio::test(flavor = "multi_thread")]
async fn a_private_answer_is_neither_shared_nor_saved() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, _) = workspace(&s).await;
    let c = general(&s, &ws);
    let (id, _) = answered(&s, &ws, &c, &mo, true).await;
    assert_eq!(s.post(&sam, &at(&ws, &c, &id, "save"), json!({})).await.0, 403, "Save of a private answer by someone other than the requester was not refused");
    assert_eq!(s.post(&mo, &at(&ws, &c, &id, "save"), json!({})).await.0, 403, "the requester saved a private answer for the team");
    for t in [&sam, &mo] {
        assert_eq!(s.post(t, &at(&ws, &c, &id, "share"), json!({ "person": "ana" })).await.0, 403, "a private answer was shared");
    }
    let files = s.k.read(|st| st.team.workspace(&ws).unwrap().files.len());
    assert_eq!(files, 0, "a private answer reached the team's files");
    s.done().await;
}

/// Save to Files puts the message in the workspace's files as Markdown; a
/// guest saves nothing.
#[tokio::test(flavor = "multi_thread")]
async fn save_puts_the_message_in_the_files_and_a_guest_cannot() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let c = general(&s, &ws);
    let id = say(&s, &sam, &ws, &c, "the launch moves to Friday").await;
    assert_eq!(s.post(&ana, &at(&ws, &c, &id, "save"), json!({})).await.0, 403, "a guest saved to the team's files");
    let (st, v) = s.post(&mo, &at(&ws, &c, &id, "save"), json!({})).await;
    assert_eq!(st, 201, "{v}");
    let name = v["name"].as_str().unwrap().to_string();
    let url = format!("{}/v1/workspaces/{ws}/files/{name}", s.base);
    let body = reqwest::Client::new().get(url).bearer_auth(&ana).send().await.unwrap().text().await.unwrap();
    assert!(body.starts_with("**Sam** · ") && body.ends_with("the launch moves to Friday\n"), "{body}");
    let (st, again) = s.post(&mo, &at(&ws, &c, &id, "save"), json!({})).await;
    assert_eq!(st, 201);
    assert_ne!(again["name"], json!(name), "saving again wrote over the first file");
    assert_eq!(s.post(&mo, &at(&ws, &c, "nothing", "save"), json!({})).await.0, 404);
    s.done().await;
}

/// Retry asks the same request again where it was asked, for who asked only.
#[tokio::test(flavor = "multi_thread")]
async fn retry_is_for_who_asked() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let c = general(&s, &ws);
    let (id, task) = answered(&s, &ws, &c, &mo, false).await;
    assert_eq!(s.post(&sam, &at(&ws, &c, &id, "retry"), json!({})).await.0, 403, "someone else retried mo's request");
    assert_eq!(s.post(&ana, &at(&ws, &c, &id, "retry"), json!({})).await.0, 403, "a guest had the agent act");
    let (st, v) = s.post(&mo, &at(&ws, &c, &id, "retry"), json!({})).await;
    assert_eq!(st, 200, "{v}");
    let again = v["task_id"].as_str().unwrap_or_else(|| panic!("retry started nothing: {v}"));
    assert_ne!(again, task);
    let asked = s.k.read(|st| st.team.run_of(again).map(|r| r.requester.clone()));
    assert_eq!(asked.as_deref(), Some("mo"));
    let ask = s.k.read(|st| {
        let w = st.team.workspace(&ws).unwrap();
        w.chat.asked.get(again).and_then(|m| w.channels[&c].messages.iter().find(|x| &x.id == m)).map(|m| m.text.clone())
    });
    assert_eq!(ask.as_deref(), Some(ASK), "retry did not ask the same words");
    s.done().await;
}
