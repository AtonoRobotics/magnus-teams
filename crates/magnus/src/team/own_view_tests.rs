//! What each person keeps of the team chat, and what they are told about.
//! sam owns the workspace, mo is a member, ana a guest.

use super::chat::Chat;
use super::{Change, Ledger, Message, Role};
use crate::admin_app::tests::{server_with, Server};
use axum::Router;
use reqwest::Method;
use serde_json::{json, Value};

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

async fn view(s: &Server, t: &str, ws: &str, channel: &str) -> Vec<Value> {
    let (st, v) = s.get(t, &format!("/v1/workspaces/{ws}/channels/{channel}/view")).await;
    assert_eq!(st, 200, "{v}");
    v["messages"].as_array().cloned().unwrap_or_default()
}

fn texts(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(|i| i["text"].as_str().map(String::from)).collect()
}

/// Only its sender deletes a message. It goes for everyone, and its text,
/// edits included, leaves the log.
#[tokio::test(flavor = "multi_thread")]
async fn a_sender_deletes_their_message_for_everyone_and_from_the_log() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, _) = workspace(&s).await;
    let c = general(&s, &ws);
    let id = say(&s, &mo, &ws, &c, "the door code is 4471").await;
    let path = format!("/v1/workspaces/{ws}/channels/{c}/messages/{id}");
    assert_eq!(s.call(&mo, Method::PUT, &path, Some(json!({ "text": "the door code is 4472" }))).await.0, 200);
    assert_eq!(s.call(&sam, Method::DELETE, &path, None).await.0, 403, "someone else deleted mo's message");
    let (st, v) = s.call(&mo, Method::DELETE, &path, None).await;
    assert_eq!(st, 200, "{v}");
    for t in [&sam, &mo] {
        assert!(!texts(&view(&s, t, &ws, &c).await).iter().any(|x| x.contains("door code")), "the deleted message is still shown");
    }
    let rows = s.k.log.of_kinds(&["team"], 500).await.unwrap();
    let seqs: Vec<i64> = rows.iter().map(|(seq, _, _, _)| *seq).collect();
    let kept = s.k.log.contents(&seqs).await.unwrap();
    assert!(!kept.values().any(|b| b.to_string().contains("door code")), "the message's text is still in the log");
    assert_eq!(s.call(&mo, Method::DELETE, &path, None).await.0, 404, "a deleted message was deleted again");
    s.done().await;
}

/// Replayed after its text was erased, a deleted message keeps its place
/// and nothing else; one whose event was erased is not read as a message.
#[test]
fn a_deleted_message_replays_as_its_place_only() {
    let at = chrono::Utc::now();
    let mut l = Ledger::default();
    l.apply(&Change::Created { workspace: "w".into(), name: "T".into(), owner: "sam".into() });
    l.apply(&Change::Channel { workspace: "w".into(), channel: "c".into(), name: "general".into() });
    l.apply_at(&Change::Message { workspace: "w".into(), channel: "c".into(), message: Message::default() }, 7);
    assert!(l.workspace("w").unwrap().channels["c"].messages.is_empty(), "an erased message came back as an empty one");
    let reply = Message { id: "r".into(), sender: "sam".into(), text: "agreed".into(), thread: Some("m".into()), at: at + chrono::Duration::seconds(1) };
    l.apply_at(&Change::Message { workspace: "w".into(), channel: "c".into(), message: reply }, 8);
    let gone = Chat::Unsent { channel: "c".into(), id: "m".into(), sender: "mo".into(), thread: None, at };
    l.apply(&Change::Chat { workspace: "w".into(), chat: gone });
    let w = l.workspace("w").unwrap();
    let ids: Vec<&str> = w.channels["c"].messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m", "r"], "the deleted message lost its place before its reply");
    assert!(w.channels["c"].messages[0].text.is_empty() && w.chat.unsent.contains("m"));
    assert_eq!(l.message_seqs("r"), [8]);
}

/// Deleting a direct conversation empties it for that person only; it comes
/// back with what is written after. A channel is put away, not deleted.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_conversation_is_for_the_one_who_deletes_it() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, _) = workspace(&s).await;
    let dm = s.post(&sam, &format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["mo"] })).await.1["id"].as_str().unwrap().to_string();
    say(&s, &mo, &ws, &dm, "lunch at noon?").await;
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{dm}/clear"), json!({})).await.0, 200);
    assert!(view(&s, &sam, &ws, &dm).await.is_empty(), "sam still sees what they deleted");
    assert_eq!(texts(&view(&s, &mo, &ws, &dm).await), ["lunch at noon?"], "mo lost their copy");
    let rail = |v: Value| v["channels"].as_array().unwrap().iter().find(|c| c["id"] == json!(dm)).cloned().unwrap();
    let mine = rail(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1);
    assert_eq!((mine["deleted"].clone(), mine["unread"].clone()), (json!(true), json!(0)), "{mine}");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    say(&s, &mo, &ws, &dm, "still on?").await;
    let mine = rail(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1);
    assert_eq!((mine["deleted"].clone(), mine["unread"].clone()), (json!(false), json!(1)), "{mine}");
    assert_eq!(texts(&view(&s, &sam, &ws, &dm).await), ["still on?"]);
    let c = general(&s, &ws);
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{c}/clear"), json!({})).await.0, 400, "a channel was deleted for one person");
    s.done().await;
}

/// Archiving puts a conversation away for its archiver only, until someone
/// else writes in it or they bring it back.
#[tokio::test(flavor = "multi_thread")]
async fn an_archived_conversation_comes_back_when_someone_writes() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, _) = workspace(&s).await;
    let c = general(&s, &ws);
    let flag = |v: Value| v["channels"].as_array().unwrap().iter().find(|x| x["id"] == json!(c)).map(|x| x["archived"].clone()).unwrap();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{c}/archive"), json!({ "on": true })).await.0, 200);
    assert_eq!(flag(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1), json!(true));
    assert_eq!(flag(s.get(&mo, &format!("/v1/workspaces/{ws}/home")).await.1), json!(false), "mo's list changed");
    say(&s, &sam, &ws, &c, "my own note").await;
    assert_eq!(flag(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1), json!(true), "their own message brought it back");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    say(&s, &mo, &ws, &c, "hello all").await;
    assert_eq!(flag(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1), json!(false), "a new message left it away");
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{c}/archive"), json!({ "on": true })).await.0, 200);
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{c}/archive"), json!({ "on": false })).await.0, 200);
    assert_eq!(flag(s.get(&sam, &format!("/v1/workspaces/{ws}/home")).await.1), json!(false));
    s.done().await;
}

/// The notices: a direct message, a mention and, where the setting is all,
/// any message; never one's own, never in a channel turned off.
#[tokio::test(flavor = "multi_thread")]
async fn notices_follow_each_channels_setting() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let first = s.get(&mo, "/v1/team/notices").await.1;
    assert!(first["notices"].as_array().unwrap().is_empty() && first["now"].is_string(), "a first look brought old news: {first}");
    let after = first["now"].as_str().unwrap().to_string();
    let c = general(&s, &ws);
    let dm = s.post(&sam, &format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["mo"] })).await.1["id"].as_str().unwrap().to_string();
    say(&s, &sam, &ws, &dm, "a word in private").await;
    say(&s, &sam, &ws, &c, "hello @mo").await;
    say(&s, &ana, &ws, &c, "plain news").await;
    say(&s, &mo, &ws, &c, "mine").await;
    let look = |t: String| {
        let path = format!("/v1/team/notices?after={}", after.replace('+', "%2B"));
        let s = &s;
        async move { s.get(&t, &path).await.1 }
    };
    let v = look(mo.clone()).await;
    let kinds: Vec<(String, String)> = v["notices"].as_array().unwrap().iter().map(|n| (n["kind"].as_str().unwrap().into(), n["preview"].as_str().unwrap().into())).collect();
    let want: Vec<(String, String)> =
        [("direct", "a word in private"), ("mention", "hello @mo"), ("message", "plain news")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(kinds, want, "{v}");
    assert_eq!(v["names"]["sam"], "Sam");
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/channels/{c}/notify"), json!({ "level": "mentions" })).await.0, 200);
    let kinds: Vec<Value> = look(mo.clone()).await["notices"].as_array().unwrap().iter().map(|n| n["kind"].clone()).collect();
    assert_eq!(kinds, [json!("direct"), json!("mention")], "a plain message in a mentions-only channel was told");
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/channels/{dm}/notify"), json!({ "level": "off" })).await.0, 200);
    let kinds: Vec<Value> = look(mo.clone()).await["notices"].as_array().unwrap().iter().map(|n| n["kind"].clone()).collect();
    assert_eq!(kinds, [json!("mention")], "a conversation turned off was told");
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/channels/{c}/read"), json!({})).await.0, 200);
    assert!(look(mo.clone()).await["notices"].as_array().unwrap().is_empty(), "what mo already read was told");
    assert!(s.k.read(|st| st.team.role(&ws, "mo")) == Some(Role::Member));
    s.done().await;
}
