//! The stage beside a conversation fits what it holds (owner, 2026-11-28:
//! "The right side bar is larger than it should be for a simple task").
//! Two conversations stay at work, and `desktop/tests/stage-column.mjs`
//! opens each in Nova at the owner's window size, the first before the
//! second starts (a running task shows on every conversation's stage):
//! - one whose chat turn started a sub-agent that is still working: the
//!   thread shows it inline, and the stage does not show it again;
//! - one that started a task still running: its tile sits in a column no
//!   wider than a tile needs and no taller than its tiles.
use magnus_invariants::model::{Conversation, Script};
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

const CHAT: &str = "talking with the person you work for in a chat";
const SUB_ASK: &str = "Compare our agent with the market's.";
const TASK_ASK: &str = "Plan the team offsite for me.";
const CHILD_GOAL: &str = "Research the following public web question quickly and concisely.";
const TASK_GOAL: &str = "Plan the team offsite: venue, dates and budget.";

fn is_chat(c: &Conversation) -> bool {
    c.body["system"].to_string().contains(CHAT)
}

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn first_message_has(c: &Conversation, text: &str) -> bool {
    c.messages().first().is_some_and(|m| m.to_string().contains(text))
}

fn contract() -> Reply {
    tool(
        "task_contract",
        json!({ "goal": TASK_GOAL, "assumptions": [], "irreversible_actions": [],
            "criteria": [{ "id": "c1", "description": "the plan is written", "check": { "tool": "sandbox_exec", "input": { "command": "true" }, "expect": [{ "pointer": "/exit_code", "equals": 0 }] } }] }),
    )
}

/// The sub-agent and the task never get an answer, so both stay at work
/// while the page is measured.
fn work_that_stays() -> Script {
    script(|c| async move {
        if first_message_has(&c, CHILD_GOAL) {
            return std::future::pending::<Reply>().await;
        }
        if is_chat(&c) {
            return match (said(&c).as_str(), c.turn()) {
                (SUB_ASK, 0) => tool("start_sub_agent", json!({ "goal": CHILD_GOAL, "tools": ["web_fetch"] })),
                (TASK_ASK, 0) => tool("create_task", json!({ "title": "Plan the team offsite", "goal": TASK_GOAL })),
                _ => Reply::Text("Started.".into()),
            };
        }
        match c.turn() {
            0 => contract(),
            _ => std::future::pending::<Reply>().await,
        }
    })
}

async fn chat(api: &Api, text: &str) -> String {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert!(st.is_success(), "{v}");
    v["thread"].as_str().unwrap().to_string()
}

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (_, v) = api.get(&format!("/v1/chat/{thread}")).await;
    v["entries"].as_array().cloned().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stage_column_fits_what_it_holds() {
    let model = FakeModel::start(work_that_stays()).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let with_sub = chat(&alice, SUB_ASK).await;
    eventually("the sub-agent shows in its thread", PATIENCE, || async { entries(&alice, &with_sub).await.iter().any(|e| e["kind"] == "sub_agents").then_some(()) }).await;
    assert!(set_password(&app, "alice", PASSWORD));
    measure(&app, "MAGNUS_SUB_THREAD", &with_sub).await;
    let with_task = chat(&alice, TASK_ASK).await;
    eventually("the task is running", PATIENCE, || async {
        let listed = alice.tasks().await;
        listed["tasks"].as_array()?.iter().any(|t| t["thread"] == with_task.as_str() && t["status"] == "running").then_some(())
    })
    .await;
    measure(&app, "MAGNUS_TASK_THREAD", &with_task).await;
}

/// Opens `thread` in the browser and measures its stage.
async fn measure(app: &Appliance, which: &str, thread: &str) {
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/stage-column.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env(which, thread)
        .env("MAGNUS_SUB_GOAL", CHILD_GOAL)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{log}");
    assert!(out.status.success(), "the stage beside the conversation does not fit what it holds:\n{log}");
}
