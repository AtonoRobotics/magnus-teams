//! What a task started from chat delivers shows in the chat (owner,
//! 2026-11-01: "Composability does not show up in chat"). A request in a
//! conversation becomes a task whose deliverable is a timeline and a table;
//! once it is done, its result in the thread carries the deliverable, and
//! `desktop/tests/chat-results.mjs` finds it drawn there, in the
//! conversation, not only behind "See the outcome".

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

const CHAT: &str = "talking with the person you work for in a chat";
const ASK: &str = "Plan a 3-day San Diego trip for two under $1,000.";
const TRIP: &str = "Plan the San Diego trip: three days, two people, under one thousand dollars.";

#[tokio::test(flavor = "multi_thread")]
async fn a_task_from_chat_shows_its_deliverable_in_the_conversation() {
    let mut m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            return match c.turn() {
                0 => tool("create_task", json!({ "title": "San Diego trip for two", "goal": TRIP })),
                _ => Reply::Text("I've started the trip plan; it posts here when it's done.".into()),
            };
        }
        if !c.body.to_string().contains(TRIP) {
            return Reply::Text(String::new());
        }
        match c.turn() {
            0 => contract_unchecked("plan the trip", "a 3-day plan under $1,000"),
            _ => tool(
                "task_complete",
                json!({
                    "summary": "3-day itinerary · $912 total",
                    "deliverable": [
                        { "type": "timeline", "events": [
                            { "when": "Day 1", "label": "Arrive and settle in", "note": "Little Italy · La Jolla Cove" },
                            { "when": "Day 2", "label": "Balboa Park and the bay", "note": "Museums · harbor cruise" },
                            { "when": "Day 3", "label": "Coronado and home", "note": "Beach morning · drive back" }
                        ] },
                        { "type": "table", "columns": ["Item", "Cost"], "rows": [["Hotel, 2 nights", "$480"], ["Food", "$260"], ["Total", "$912"]] }
                    ]
                }),
            ),
        }
    }))
    .await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/chat", json!({ "text": ASK })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let trip = eventually("the trip task", PATIENCE, || async { alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == TRIP).cloned() }).await;
    let id = trip["task_id"].as_str().unwrap().to_string();
    let review = alice.wait_item(&id, "review").await;
    assert!(alice.respond(review["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.is_success());
    assert_eq!(alice.wait_settled(&id).await["status"], "completed");

    // The thread's result for the trip carries what it delivered.
    let result = eventually("the trip's result in the thread", PATIENCE, || async {
        let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
        c["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "result" && e["task_id"] == id).cloned()
    })
    .await;
    let view = result["view"].as_array().unwrap_or_else(|| panic!("the result in the thread has no deliverable: {result}"));
    assert_eq!(view.iter().map(|c| c["type"].as_str().unwrap()).collect::<Vec<_>>(), ["timeline", "table"], "{result}");

    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/chat-results.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_THREAD", &thread)
        .env("MAGNUS_TASK", &id)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the deliverable is not drawn in the chat:\n{log}");
}
