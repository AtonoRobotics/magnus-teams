//! Whose team content is: the Rust rule (`team::owner`, applied as each
//! event is written) and the SQL rule (`log::CONTENT_OWNERS`, applied to
//! rows that do not say, at `migrate` and after a restore) give the same
//! person for every event, so an erasure finds the same rows either way.

use crate::admin_app::tests::server_with;
use axum::Router;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const AT: &str = "2026-11-13T09:00:00Z";

/// One team event of every kind that carries content, with whose it
/// should be. Before, a message, an edit, a reaction, a mark, a note, a
/// request's channel and a whisper were no one's, so erasing their person
/// left them in the log.
fn events() -> Vec<(Value, Option<&'static str>)> {
    let team = |mut v: Value| {
        v["kind"] = json!("team");
        v["workspace"] = json!("ws");
        v
    };
    let chat = |c: Value| team(json!({ "change": "chat", "chat": c }));
    let message = |id: &str, sender: &str, text: &str| team(json!({ "change": "message", "channel": "c", "message": { "id": id, "sender": sender, "text": text, "at": AT } }));
    vec![
        (team(json!({ "change": "created", "name": "W", "owner": "sam" })), None),
        (team(json!({ "change": "channel", "channel": "c", "name": "general" })), None),
        (message("m1", "mo", "mo's door code"), Some("mo")),
        (message("m2", "agent:mo", "posted on mo's request"), Some("mo")),
        (team(json!({ "change": "message", "channel": "c", "message": { "id": "m3", "sender": "sam", "text": "sam replies", "thread": "m1", "at": AT } })), Some("sam")),
        (chat(json!({ "what": "edited", "channel": "c", "id": "m1", "text": "mo's new door code", "at": AT })), Some("mo")),
        (chat(json!({ "what": "edited", "channel": "c", "id": "m2", "text": "posted again", "at": AT })), Some("mo")),
        (chat(json!({ "what": "edited", "channel": "c", "id": "m3", "text": "sam replies again", "at": AT })), Some("sam")),
        (chat(json!({ "what": "reacted", "channel": "c", "id": "m1", "emoji": "+1", "person": "ana", "on": true })), Some("ana")),
        (chat(json!({ "what": "notify", "channel": "c", "person": "mo", "level": "mentions" })), Some("mo")),
        (chat(json!({ "what": "read", "channel": "c", "person": "mo", "at": AT })), Some("mo")),
        (chat(json!({ "what": "archived", "channel": "c", "person": "mo", "on": true, "at": AT })), Some("mo")),
        (chat(json!({ "what": "cleared", "channel": "c", "person": "mo", "at": AT })), Some("mo")),
        (chat(json!({ "what": "pinned", "channel": "c", "id": "m3", "pinned": true })), None),
        (chat(json!({ "what": "unsent", "channel": "c", "id": "m3", "sender": "sam", "at": AT })), None),
        (team(json!({ "change": "note", "task": "t1", "note": { "id": "n1", "text": "mo's scratch", "by": "agent:mo", "at": AT } })), Some("mo")),
        (team(json!({ "change": "run", "channel": "c", "thread": null, "task": "t1", "requester": "mo", "history": "mo: mo's door code" })), Some("mo")),
        (team(json!({ "change": "whisper", "channel": "c", "thread": null, "task": "t2", "owner": "mo", "history": "mo: a whisper" })), Some("mo")),
        (team(json!({ "change": "held", "person": "mo", "file": { "name": "mo.csv", "size": 1, "by": "mo" } })), Some("mo")),
        (team(json!({ "change": "released", "person": "mo", "name": "mo.csv" })), Some("mo")),
        (team(json!({ "change": "file", "file": { "name": "plan.md", "size": 1, "by": "mo" } })), None),
    ]
}

async fn owners(c: &tokio_postgres::Client) -> BTreeMap<i64, Option<String>> {
    let rows = c.query("SELECT seq, person FROM public.content ORDER BY seq", &[]).await.unwrap();
    rows.iter().map(|r| (r.get(0), r.get(1))).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rust_and_sql_rules_give_every_team_event_the_same_owner() {
    let s = server_with("", |_| Router::new()).await;
    let mut want = BTreeMap::new();
    for (e, who) in events() {
        let ev = serde_json::from_value(e.clone()).unwrap_or_else(|err| panic!("{e}: {err}"));
        let written = s.k.emit("person:sam", ev).await.unwrap();
        want.insert(written.seq, (e, who.map(String::from)));
    }
    let c = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    let rust = owners(&c).await;
    for (seq, (e, who)) in &want {
        assert_eq!(rust.get(seq), Some(who), "written, {e} is {:?}'s, not {who:?}'s", rust.get(seq));
    }
    c.batch_execute("UPDATE public.content SET person = NULL").await.unwrap();
    c.batch_execute(crate::log::CONTENT_OWNERS).await.unwrap();
    let sql = owners(&c).await;
    for (seq, owner) in &rust {
        let what = want.get(seq).map_or_else(|| format!("event {seq}"), |(e, _)| e.to_string());
        assert_eq!(sql.get(seq), Some(owner), "the SQL rule says {what} is {:?}'s, the Rust rule {owner:?}'s", sql.get(seq));
    }
    s.done().await;
}
