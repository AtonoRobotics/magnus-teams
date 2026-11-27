//! A page Magnus delivers runs as a page in the conversation (owner,
//! 2026-11-26: "interactive dashboard was code in a scrolling frame"). A
//! request in a conversation becomes a task that writes an HTML dashboard
//! and delivers it; `desktop/tests/html-deliverable.mjs` finds the page
//! drawn live in the thread, its script run, and none of its source shown.

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

const CHAT: &str = "talking with the person you work for in a chat";
const ASK: &str = "Build an interactive summary dashboard from my recent project files.";
const GOAL: &str = "Build the project dashboard as one HTML page.";
/// A page delivered as a file only, outside the deliverable's view: its card
/// runs it, with nothing to open (owner, 2026-11-27: "You have to open the
/// file. It should have just opened.").
const CARD_PAGE: &str = "<!doctype html><html><body><p id=\"card\">Not run</p><script>document.getElementById('card').textContent='Card page drawn live';</script></body></html>";
const PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Projects</title></head><body><h1 id=\"state\">Not run</h1><button id=\"more\">More</button><script>document.getElementById('state').textContent='Dashboard drawn live';</script></body></html>";

#[tokio::test(flavor = "multi_thread")]
async fn a_delivered_page_runs_live_in_the_conversation() {
    let mut m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            return match c.turn() {
                0 => tool("create_task", json!({ "title": "Project dashboard", "goal": GOAL })),
                _ => Reply::Text("The dashboard posts here when it's done.".into()),
            };
        }
        if !c.body.to_string().contains(GOAL) {
            return Reply::Text(String::new());
        }
        match c.turn() {
            0 => plan("build the dashboard"),
            1 => tool("sandbox_write_file", json!({ "path": "project-dashboard.html", "content": PAGE })),
            2 => tool("deliver_file", json!({ "path": "project-dashboard.html" })),
            3 => tool("sandbox_write_file", json!({ "path": "status-board.html", "content": CARD_PAGE })),
            4 => tool("deliver_file", json!({ "path": "status-board.html" })),
            _ => tool(
                "task_complete",
                json!({
                    "summary": "One page with every project and its next step.",
                    "deliverable": [
                        { "type": "file", "file": "project-dashboard.html" },
                        { "type": "stat", "label": "Dashboard entries", "value": 19 }
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
    let task = eventually("the dashboard task", PATIENCE, || async { alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == GOAL).cloned() }).await;
    let id = task["task_id"].as_str().unwrap().to_string();
    assert_eq!(alice.wait_settled(&id).await["status"], "completed");
    eventually("the dashboard's result in the thread", PATIENCE, || async {
        let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
        let entries = c["entries"].as_array().cloned().unwrap_or_default();
        let at = entries.iter().position(|e| e["kind"] == "result" && e["task_id"] == id && e["view"].is_array())?;
        (entries.len() > at + 1).then_some(())
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/html-deliverable.mjs"))
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
    assert!(out.status.success(), "the delivered page does not run in the chat:\n{log}");
}
