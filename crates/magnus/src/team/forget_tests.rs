//! Erasing a person deletes their team messages for everyone, from the log
//! as well as the channels, and leaves everyone else's words as they were.
//! sam owns the workspace and mo, a member from the config, is erased.

use crate::admin_app::tests::{server_with, Server};
use axum::Router;
use reqwest::Method;
use serde_json::json;

async fn say(s: &Server, t: &str, ws: &str, channel: &str, text: &str, thread: Option<&str>) -> String {
    let (st, v) = s.post(t, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": text, "thread": thread })).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap_or_else(|| panic!("no id: {v}")).to_string()
}

/// Before, the erasure kept mo's messages in the shared channel, words
/// and edits, in the state and in the log.
#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_person_deletes_their_team_messages_and_leaves_everyone_elses() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    let mo = s.token("mo").await;
    let c = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.iter().find(|(_, c)| c.name == "general").map(|(id, _)| id.clone()).unwrap());
    let theirs = say(&s, &mo, &ws, &c, "the door code is 4471", None).await;
    let path = format!("/v1/workspaces/{ws}/channels/{c}/messages/{theirs}");
    assert_eq!(s.call(&mo, Method::PUT, &path, Some(json!({ "text": "the door code is 4472" }))).await.0, 200);
    let reply = say(&s, &sam, &ws, &c, "sam answers: it opens", Some(&theirs)).await;
    let own = say(&s, &sam, &ws, &c, "sam's own news", None).await;

    s.k.erase_person("person:sam", "mo").await.unwrap();

    let w = s.k.read(|st| st.team.workspace(&ws).unwrap().clone());
    let messages = &w.channels[&c].messages;
    let gone = messages.iter().find(|m| m.id == theirs).expect("the deleted message lost its place, so its reply lost its thread");
    assert!(gone.text.is_empty() && gone.sender.is_empty() && w.chat.unsent.contains(&theirs), "mo's message is still in the channel: {gone:?}");
    assert!(!w.chat.edited.contains(&theirs), "mo's edit is still marked");
    let text_of = |id: &str| messages.iter().find(|m| m.id == id).map(|m| m.text.clone());
    assert_eq!(text_of(&reply).as_deref(), Some("sam answers: it opens"), "sam's reply lost its words");
    assert_eq!(text_of(&own).as_deref(), Some("sam's own news"), "sam's message lost its words");
    assert_eq!(messages.iter().find(|m| m.id == reply).and_then(|m| m.thread.clone()), Some(theirs.clone()), "the reply left its thread");

    let (st, v) = s.get(&sam, &format!("/v1/workspaces/{ws}/channels/{c}/view")).await;
    assert_eq!(st, 200, "{v}");
    assert!(!v.to_string().contains("door code"), "the channel still shows mo's words: {v}");
    assert!(v.to_string().contains("sam answers"), "the channel lost sam's reply: {v}");

    let rows = s.k.log.of_kinds(&["team"], 500).await.unwrap();
    let seqs: Vec<i64> = rows.iter().map(|(seq, _, _, _)| *seq).collect();
    let kept = s.k.log.contents(&seqs).await.unwrap();
    assert!(!kept.values().any(|b| b.to_string().contains("door code")), "mo's words, or an edit of them, are still in the log");
    assert!(kept.values().any(|b| b.to_string().contains("sam answers")), "sam's reply left the log with mo's message");
    s.done().await;
}
