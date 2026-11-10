//! Tasks at volume: a batch of 20 requests (questions, chat messages that
//! start several tasks, typed tasks, schedules), driven in a real browser
//! (`desktop/tests/volume.mjs`). Tasks are grouped by the request that
//! started them, in sections ordered Needs you, Running, Waiting, Done,
//! Failed; actionable rows lead and remain reachable in the independently
//! scrolling work region without hiding its header or the composer (UX-01). A message that starts tasks shows one card, and their results
//! come back as one-liners.

use magnus_invariants::appliance::workspace_root;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";

/// What a task in the batch does, named by a marker in its text.
const KINDS: [&str; 6] = ["done", "confirm", "question", "wait", "fail", "running"];

fn kind_of(c: &Conversation) -> &'static str {
    let first = c.messages().first().map(|m| m.to_string()).unwrap_or_default();
    KINDS.iter().find(|k| first.contains(&format!("MARK-{k}"))).copied().unwrap_or("done")
}

/// A task's replies by turn: a contract checked against a page, then the end
/// its marker names. Checking is internal (spec rev 140), so what needs the
/// person is a question: "confirm" asks one before the work.
fn task_reply(kind: &str, turn: usize, page: &str) -> Reply {
    let criterion = json!({ "id": "c1", "description": "The source still says ok", "check": { "tool": "web_fetch", "input": { "url": page }, "expect": [{ "pointer": "/text", "contains": "ok" }] } });
    let contract = tool("task_contract", json!({ "title": "Typed task", "goal": "do it", "assumptions": [], "irreversible_actions": [], "criteria": [criterion] }));
    match (kind, turn) {
        ("question", _) => tool("ask_principal", json!({ "question": "Which quarter?" })),
        ("confirm", _) => tool("ask_principal", json!({ "question": "Is this the tone you want?" })),
        ("fail", _) => tool("task_fail", json!({ "reason": "The source is gone." })),
        ("wait", _) => tool("wait_until", json!({ "at": "2099-01-01T00:00:00Z", "reason": "the filing window opens" })),
        (_, 0) => contract,
        _ => tool(
            "task_complete",
            json!({ "summary": "Finished.\nThe details are in the task.", "deliverable": [{ "type": "text", "text": "Finished.\nThe details are in the task." }] }),
        ),
    }
}

/// The batch: (what the person writes, where, the tasks a chat message
/// starts as (title, kind)).
fn batch() -> Vec<(&'static str, &'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        ("What's the difference between an LLC and an S-corp?", "chat", vec![]),
        ("How many ounces are in a liter?", "chat", vec![]),
        ("Who wrote The Remains of the Day?", "chat", vec![]),
        ("What does EBITDA stand for?", "chat", vec![]),
        ("What time zone is Lisbon in?", "chat", vec![]),
        ("What is a 1099-NEC?", "chat", vec![]),
        (
            "Plan the team offsite.",
            "chat",
            vec![
                ("Shortlist venues", "done"),
                ("Draft the agenda", "done"),
                ("Collect dietary needs", "done"),
                ("Write the invitation", "confirm"),
                ("Pick the dates", "question"),
            ],
        ),
        (
            "Close out the Q3 books.",
            "chat",
            vec![("Reconcile bank accounts", "done"), ("Accrue open invoices", "done"), ("Pull the payroll export", "fail"), ("File the sales tax", "wait")],
        ),
        ("Research three CRM tools.", "chat", vec![("Compare HubSpot", "done"), ("Compare Pipedrive", "done"), ("Compare Attio", "done")]),
        ("Prepare the board update.", "chat", vec![("Gather KPIs", "running"), ("Draft the narrative", "confirm")]),
        ("Renew the domain.", "chat", vec![("Renew magnus domain", "question")]),
        (
            "Onboard the new hire.",
            "chat",
            vec![
                ("Create accounts", "done"),
                ("Order a laptop", "done"),
                ("Book intro meetings", "done"),
                ("Share the handbook", "done"),
                ("Write the welcome note", "confirm"),
                ("Set up payroll", "running"),
            ],
        ),
        ("Summarise the support tickets from last week. MARK-done", "task", vec![]),
        ("Draft the customer newsletter. MARK-confirm", "task", vec![]),
        ("Export the old CRM data. MARK-fail", "task", vec![]),
        ("Choose a new office printer. MARK-question", "task", vec![]),
        ("Every Monday, summarise my open invoices.", "chat", vec![]),
        ("Each morning, list my meetings.", "chat", vec![]),
        ("Tidy the shared drive.", "chat", vec![("Find duplicate files", "confirm"), ("Archive old projects", "confirm"), ("Fix folder names", "done")]),
        ("Write the release notes. MARK-done", "task", vec![]),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_of_requests_prioritises_actionable_tasks_without_page_scrolling() {
    let web = FakeWeb::start(&[("/source", "ok")]).await;
    let page = web.url("/source");
    let starts: std::collections::HashMap<&str, Vec<(&str, &str)>> = batch().into_iter().map(|(t, _, s)| (t, s)).collect();
    let mut m = FakeModel::start(script(move |c| {
        let (starts, page) = (starts.clone(), page.clone());
        async move {
            if c.body["system"].to_string().contains(CHAT) {
                let said = c.messages().first().and_then(|m| m["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str().map(String::from))).unwrap_or_default();
                if c.turn() > 0 {
                    return Reply::Text("Started.".into());
                }
                if said.starts_with("Every") || said.starts_with("Each") {
                    return tool("create_schedule", json!({ "title": "Weekly invoice summary", "goal": said, "every_minutes": 10080 }));
                }
                return match starts.get(said.as_str()).filter(|s| !s.is_empty()) {
                    Some(s) => tools(s.iter().map(|(title, kind)| ("create_task", json!({ "title": title, "goal": format!("{title}. MARK-{kind}") }))).collect()),
                    None => Reply::Text("Here is the answer.".into()),
                };
            }
            let kind = kind_of(&c);
            if kind == "running" && c.turn() > 0 {
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
            task_reply(kind, c.turn(), &page)
        }
    }))
    .await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, PERSON, PASSWORD));
    let alice = app.api(PERSON);

    let mut offsite = String::new();
    for (text, place, _) in batch() {
        if place == "task" {
            alice.submit(text).await;
            continue;
        }
        let (st, v) = alice.post("/v1/chat", json!({ "text": text })).await;
        assert_eq!(st.as_u16(), 201, "{v}");
        alice.wait_settled(v["task_id"].as_str().unwrap()).await;
        if text.starts_with("Plan the team offsite") {
            offsite = v["thread"].as_str().unwrap().to_string();
        }
    }
    // Every task reaches the state its marker names (running ones stay running).
    let want = |t: &Value| -> &str {
        let text = t["text"].as_str().unwrap_or_default();
        match KINDS.iter().find(|k| text.contains(&format!("MARK-{k}"))).copied().unwrap_or("done") {
            "done" => "done",
            "confirm" | "question" => "needs",
            "wait" => "waiting",
            "fail" => "failed",
            _ => "running",
        }
    };
    let state = |t: &Value| -> &str {
        match (t["needs_you"].as_bool(), t["status"].as_str()) {
            (Some(true), _) => "needs",
            (_, Some("running")) => "running",
            (_, Some("suspended")) => "waiting",
            (_, Some("completed")) => "done",
            _ => "failed",
        }
    };
    let tasks = eventually("every task in its intended state", PATIENCE, || {
        let alice = alice.clone();
        async move {
            let v = alice.tasks().await["tasks"].as_array().cloned().unwrap_or_default();
            (v.len() == 29 && v.iter().all(|t| state(t) == want(t))).then_some(v)
        }
    })
    .await;
    let needs = tasks.iter().filter(|t| t["needs_you"] == true).count();
    assert_eq!(needs, 9);
    // Tasks from one message share its group; typed tasks stand alone.
    let in_offsite: Vec<&Value> = tasks.iter().filter(|t| t["group"]["title"] == "Plan the team offsite.").collect();
    assert_eq!(in_offsite.len(), 5);
    assert!(in_offsite.iter().all(|t| t["group"]["kind"] == "chat" && t["group"]["thread"] == offsite.as_str()));
    assert!(tasks.iter().filter(|t| t["text"].as_str().unwrap().contains("Draft the customer newsletter")).all(|t| t["group"].is_null()));
    assert!(tasks.iter().any(|t| t["title"] == "Typed task"), "a typed task did not take its contract's title");
    // The schedules asked for are set up at once, so the Schedules view has them.
    assert!(!alice.get("/v1/schedules").await.1["schedules"].as_array().unwrap().is_empty(), "a schedule asked for in chat was not set up");

    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/volume.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", PERSON)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_NEEDS", needs.to_string())
        .env("MAGNUS_THREAD", &offsite)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the volume check failed:\n{log}");
}
