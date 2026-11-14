//! The thread route's JSON, pinned: what the desktop app's chat view
//! (desktop/ui/chat-view.js, personal-chat.js) reads. A fixture thread with
//! every kind of entry (an opener, a message, an answer, the tasks a turn
//! started and their result, sub-agents, an app's card, suggestion cards
//! open and declined, a live turn, a reaction, what the person may do) and
//! two branches (one made now, one stored in an older form) is read through
//! `GET /v1/chat/{thread}` and compared, field for field, with
//! `chat_thread_golden.json`. Times are replaced by `<at>`.
use crate::events::Ev;
use serde_json::{json, Value};

const GOLDEN: &str = include_str!("golden.json");

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

fn created(task: &str, run: &str, text: &str, chat: Value) -> Ev {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(json!({ "kind": "task_created", "task": task, "run": run, "person": "mo", "text": text, "config": config, "chat": chat }))
}

fn card(run: &str, call: &str, data: Value) -> Ev {
    ev(
        json!({ "kind": "tool_result", "run": run, "call_id": call, "status": "ok", "text": "", "is_error": false, "label": { "untrusted": false, "private": false }, "data": data }),
    )
}

/// Every string that is a time becomes `<at>`: the log stamps events with
/// the time they are written.
fn timeless(v: &mut Value) {
    match v {
        Value::String(s) if s.parse::<chrono::DateTime<chrono::Utc>>().is_ok() => *v = json!("<at>"),
        Value::Array(a) => a.iter_mut().for_each(timeless),
        Value::Object(o) => o.values_mut().for_each(timeless),
        _ => {}
    }
}

/// The fixture: Mo's thread `th1` and an opener's `th2`, then a branch of
/// `th1` at its answer and a branch stored before entries carried `may`.
pub(crate) async fn fixture(k: &crate::kernel::Kernel) {
    let mo = "person:mo";
    let evs = vec![
        created("t1", "r1", "Plan my trip to Lisbon", json!({ "thread": "th1", "turn": true })),
        card("r1", "c1", json!({ "card": "card1", "suggestion": true, "title": "Book the flights", "text": "Book flights to Lisbon", "plan": "Find and book the flights" })),
        card("r1", "c2", json!({ "card": "card2", "suggestion": true, "title": "Watch prices", "text": "Watch the fares", "every_minutes": 1440 })),
        card("r1", "c3", json!({ "card": "card3", "connector": "no_such_app", "why": "to read the booking mail" })),
        ev(json!({ "kind": "card_declined", "card": "card2", "person": "mo", "reason": "not needed" })),
        created("t2", "r2", "Find flights to Lisbon", json!({ "thread": "th1", "turn": false, "from_run": "r1" })),
        ev(json!({ "kind": "completed", "run": "r2", "summary": "Found three flights." })),
        created("t3", "r3", "Look up hotels", Value::Null),
        ev(json!({ "kind": "sub_agent", "child": "r3", "parent": "r1", "tools": ["web_fetch"] })),
        ev(json!({ "kind": "completed", "run": "r3", "summary": "Two hotels near the river." })),
        ev(json!({ "kind": "completed", "run": "r1", "summary": "Here is your plan." })),
        created("t4", "r4", "And the hotel?", json!({ "thread": "th1", "turn": true })),
        ev(json!({ "kind": "reacted", "person": "mo", "thread": "th1", "entry": "a:t1", "reaction": "great" })),
        created("t5", "r5", "Mail me about new invoices", json!({ "thread": "th2", "turn": true, "opener": { "app": "mail", "app_title": "Mail" } })),
        ev(json!({ "kind": "completed", "run": "r5", "summary": "I will watch for invoices." })),
        ev(json!({
            "kind": "thread_branched", "person": "mo", "thread": "old-branch", "from": "th0", "upto": "a:t0", "title": "Old",
            "entries": [
                { "id": "m:t0", "kind": "message", "text": "Old words", "at": "2026-11-01T10:00:00Z", "task_id": "t0", "view": null, "files": [] },
                { "id": "a:t0", "kind": "answer", "text": "Old answer", "at": "2026-11-01T10:01:00Z", "task_id": "t0", "spoken": null },
                { "id": "x:t0", "kind": "some_retired_kind", "text": "kept as it was" },
            ],
        })),
    ];
    for e in evs {
        k.emit(mo, e).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_thread_route_sends_what_the_app_reads() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    // The person's words are recalled through the content store's walls, as `magnus migrate` sets them up.
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    fixture(&s.k).await;
    let mo = s.token("mo").await;
    let get = reqwest::Method::GET;
    let (code, branched) = s.call(&mo, reqwest::Method::POST, "/v1/chat/th1/entries/a:t1/branch", None).await;
    assert_eq!(code, 201, "{branched}");
    let branch = branched["thread"].as_str().unwrap().to_string();
    let mut got = json!({});
    for (name, path) in
        [("th1", "/v1/chat/th1".to_string()), ("th2", "/v1/chat/th2".into()), ("branch", format!("/v1/chat/{branch}")), ("old_branch", "/v1/chat/old-branch".into())]
    {
        let (code, mut body) = s.call(&mo, get.clone(), &path, None).await;
        assert_eq!(code, 200, "{path}: {body}");
        if name == "branch" {
            body["thread"] = json!("<branch>");
        }
        got[name] = body;
    }
    timeless(&mut got);
    let want: Value = serde_json::from_str(GOLDEN).unwrap();
    assert!(got == want, "the thread route's JSON changed; it is now:\n{}", serde_json::to_string_pretty(&got).unwrap());
    s.done().await;
}
