//! The library and projects. A file uploaded into a project is used by a
//! task started there, and what the task delivers lands in the library under
//! that project; the project's instructions reach the task as the person's
//! own words, and its files are read on demand with their taint. After
//! reading an uploaded file, a task's web lookups ask once, and one
//! approval covers them all; chat asks once per conversation; a question
//! that read nothing private never asks.

use base64::Engine;
use magnus_invariants::appliance::workspace_root;
use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::sync::Arc;

const CHAT: &str = "talking with the person you work for in a chat";
const PRICES: &str = "item,price\nwidget,40\nbolt,2\n";
const INSTRUCTIONS: &str = "Prices are in euros. Keep reports to one page.";

fn is_chat(c: &Conversation) -> bool {
    c.body["system"].to_string().contains(CHAT)
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

/// Chat turns follow `chat` by turn; tasks follow `task` by turn.
fn model(chat: Vec<Reply>, task: Vec<Reply>) -> Script {
    let (chat, task) = (Arc::new(chat), Arc::new(task));
    script(move |c| {
        let (chat, task) = (chat.clone(), task.clone());
        async move {
            let s = if is_chat(&c) { &chat } else { &task };
            s.get(c.turn()).cloned().unwrap_or_else(|| tool("task_fail", json!({ "reason": "script exhausted" })))
        }
    })
}

fn contract(goal: &str, command: &str) -> Reply {
    tool(
        "task_contract",
        json!({ "title": goal, "goal": goal, "assumptions": [], "irreversible_actions": [],
            "criteria": [{ "id": "c1", "description": "the report is right", "check": { "tool": "sandbox_exec", "input": { "command": command }, "expect": [{ "pointer": "/exit_code", "equals": 0 }] } }] }),
    )
}

async fn new_project(api: &Api, name: &str, instructions: &str) -> String {
    let (st, v) = api.post("/v1/projects", json!({ "name": name, "instructions": instructions })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    v["id"].as_str().unwrap().to_string()
}

async fn upload(api: &Api, name: &str, media: &str, content: &str, where_: Value) -> String {
    let mut body = json!({ "name": name, "media_type": media, "data_b64": b64(content) });
    for (k, v) in where_.as_object().into_iter().flatten() {
        body[k] = v.clone();
    }
    let (st, v) = api.post("/v1/files", body).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    v["id"].as_str().unwrap().to_string()
}

async fn files(api: &Api, query: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/files{query}")).await;
    assert!(st.is_success(), "{v}");
    v["files"].as_array().cloned().unwrap_or_default()
}

fn names(v: &[Value]) -> Vec<String> {
    let mut n: Vec<String> = v.iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
    n.sort();
    n
}

/// A search service that answers every query with the given pages.
async fn fake_search(pages: Vec<String>) -> String {
    let app =
        axum::Router::new().route(
            "/search",
            axum::routing::get(move || {
                let pages = pages.clone();
                async move {
                    axum::Json(json!({ "results": pages.iter().map(|p| json!({ "title": "Widget prices", "url": p, "content": "Widgets from 38 euros." })).collect::<Vec<_>>() }))
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/search", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

async fn with_search(app: &mut Appliance, search: String) {
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    cfg["web"].as_table_mut().unwrap().insert("search_url".into(), toml::Value::String(search));
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
}

/// The owner's test (2026-10-29): in the app, upload a file into a project,
/// start a task there that uses it, and find its output in the library
/// under that project. The browser does what the person does
/// (`desktop/tests/library.mjs`); the log is checked here.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_uploaded_into_a_project_is_used_by_a_task_there_and_its_output_lands_in_the_library_under_that_project() {
    let report = "# Kitchen remodel prices\n\n| Item | Price |\n|---|---|\n| widget | 40 |\n| bolt | 2 |\n\nTotal: 42 EUR\n";
    let task = vec![
        contract("Total the prices", "grep -q 'Total: 42 EUR' /workspace/report.md"),
        tool("project_read", json!({ "name": "prices.csv" })),
        tool("sandbox_write_file", json!({ "path": "report.md", "content": report })),
        tool("deliver_file", json!({ "path": "report.md" })),
        tool("task_complete", json!({ "summary": "Totalled the two prices: 42 euros.", "deliverable": [{ "type": "file", "file": "report.md" }] })),
    ];
    // Asked for in the project's conversation, the work becomes a task there.
    let task = Arc::new(task);
    let s = script(move |c| {
        let task = task.clone();
        async move {
            if !is_chat(&c) {
                return task.get(c.turn()).cloned().unwrap_or_else(|| tool("task_fail", json!({ "reason": "script exhausted" })));
            }
            // Only the person's message starts the task: the turn that returns
            // its outcome names it too (decisions/2026-11-09-outcomes-return-to-the-agent.md).
            let first = c.messages().first().map(|m| m.to_string()).unwrap_or_default();
            let asked = first.contains("Total the prices") && !first.contains("This turn is not a message from the person");
            match (asked, c.turn()) {
                (true, 0) => tool("create_task", json!({ "title": "Total the prices", "goal": "Total the prices in prices.csv." })),
                (true, _) => Reply::Text("Started; it shows here as it goes.".into()),
                _ => Reply::Text("Noted.".into()),
            }
        }
    });
    let mut m = FakeModel::start(s).await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, PERSON, PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/library.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", PERSON)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_PRICES", PRICES)
        .env("MAGNUS_INSTRUCTIONS", INSTRUCTIONS)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the library check failed:\n{log}");

    let alice = app.api(PERSON);
    let (_, ps) = alice.get("/v1/projects").await;
    let project = ps["projects"].as_array().unwrap().iter().find(|p| p["name"] == "Kitchen remodel").expect("the project").clone();
    let pid = project["id"].as_str().unwrap().to_string();
    let (_, p) = alice.get(&format!("/v1/projects/{pid}")).await;
    assert_eq!(p["instructions"], INSTRUCTIONS);
    let tasks = p["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "{p}");
    let tid = tasks[0]["task_id"].as_str().unwrap().to_string();
    let t = alice.task(&tid).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(t["project"]["id"], pid.as_str());

    // The task got the project's instructions as the person's words, its
    // files as a list to read on demand, and read the upload with its taint.
    let first = m.requests().into_iter().find(|c| !is_chat(c)).unwrap();
    let opening = first.messages()[0].to_string();
    assert!(opening.contains(INSTRUCTIONS), "the instructions did not reach the task: {opening}");
    assert!(opening.contains("prices.csv") && !opening.contains("widget,40"), "the reference file was not listed, or was put in the context whole: {opening}");
    let offered = first.offered_tools();
    assert!(offered.contains(&"project_read".to_string()), "{offered:?}");
    assert!(!offered.iter().any(|t| t.starts_with("project_") && t != "project_read"), "a tool can change the project: {offered:?}");
    let read = m.requests().into_iter().find(|c| !is_chat(c) && c.turn() == 2).unwrap().last_results();
    assert!(read.len() == 1 && !read[0].is_error && read[0].text.contains("widget,40"), "project_read did not return the file: {read:?}");
    assert_eq!(t["taint"], json!({ "untrusted": true, "private": true }), "the upload's taint did not reach the task");

    // In the library under the project: the upload, and what the task delivered, linked to it.
    let inside = files(&alice, &format!("?project={pid}")).await;
    assert!(names(&inside).contains(&"prices.csv".to_string()) && names(&inside).contains(&"report.md".to_string()), "{inside:?}");
    let rep = inside.iter().find(|f| f["name"] == "report.md").unwrap();
    assert_eq!(rep["source"], "delivered");
    assert_eq!(rep["origin"]["task_id"], tid.as_str());
    let (st, _) = alice.get(&format!("/v1/files/{}", rep["id"].as_str().unwrap())).await;
    assert!(st.is_success());
    // Search reaches names and text content.
    assert_eq!(names(&files(&alice, "?q=widget").await), ["prices.csv", "report.md"]);
    assert_eq!(names(&files(&alice, "?q=bathroom").await), ["notes.txt"]);
    // The conversations started in the project are in it: the one that asked
    // for the work, and the one with the file sent with its message.
    let threads: Vec<&str> = p["threads"].as_array().unwrap().iter().filter_map(|t| t["title"].as_str()).collect();
    assert_eq!(threads, ["Keep these notes for later.", "Total the prices in prices.csv."], "{p}");
    let notes = files(&alice, "?q=notes").await;
    assert_eq!(notes[0]["origin"]["kind"], "chat");
    assert_eq!(notes[0]["project"]["id"], pid.as_str());

    // Moved out, the task takes what it delivered with it; a file moves on its own.
    assert_eq!(alice.post("/v1/moves", json!({ "item": tid, "project": null })).await.0.as_u16(), 204);
    assert_eq!(names(&files(&alice, &format!("?project={pid}")).await), ["notes.txt", "prices.csv"]);
    assert_eq!(alice.post("/v1/moves", json!({ "item": rep["id"], "project": pid })).await.0.as_u16(), 204);
    assert_eq!(names(&files(&alice, &format!("?project={pid}")).await), ["notes.txt", "prices.csv", "report.md"]);
    // Projects and files are their person's alone.
    let bob = app.api("bob");
    assert_eq!(bob.get(&format!("/v1/projects/{pid}")).await.0.as_u16(), 404);
    assert_eq!(bob.get(&format!("/v1/files/{}", rep["id"].as_str().unwrap())).await.0.as_u16(), 404);
    assert!(files(&bob, "").await.is_empty());
    assert_eq!(bob.post("/v1/moves", json!({ "item": tid, "project": null })).await.0.as_u16(), 404);
}

/// The owner's decision (2026-10-29): a task that read an uploaded file and
/// then looks prices up on the web asks once, and that approval covers all
/// its web lookups; it then runs to completion.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_task_that_reads_an_uploaded_file_then_searches_prices_asks_once_and_completes() {
    let web = FakeWeb::start(&[("/widgets", "Widget: 38 EUR"), ("/bolts", "Bolt: 2 EUR")]).await;
    let (widgets, bolts) = (web.url("/widgets"), web.url("/bolts"));
    let search = fake_search(vec![widgets.clone()]).await;
    let task = vec![
        contract("Compare prices", "true"),
        tool("project_read", json!({ "name": "prices.csv" })),
        tool("web_search", json!({ "query": "widget price" })),
        tool("web_fetch", json!({ "url": widgets })),
        tool("web_fetch", json!({ "url": bolts })),
        tool(
            "task_complete",
            json!({ "summary": "Compared.", "deliverable": [{ "type": "table", "columns": ["Item", "Ours", "Online"], "rows": [["widget", 40, 38], ["bolt", 2, 2]] }] }),
        ),
    ];
    let m = FakeModel::start(model(vec![], task)).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    with_search(&mut app, search).await;
    let alice = app.api("alice");
    let pid = new_project(&alice, "Kitchen remodel", INSTRUCTIONS).await;
    upload(&alice, "prices.csv", "text/csv", PRICES, json!({ "project": pid })).await;
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Check our prices against what shops charge online.", "project": pid })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["task_id"].as_str().unwrap().to_string();

    let ask = alice.wait_item(&id, "approval").await;
    assert_eq!(ask["details"]["tool"], "web_search");
    assert_eq!(ask["details"]["web_lookups"], "task");
    assert_eq!(ask["summary"], "This task reads your project files and searches the web — allow web lookups for this task?");
    assert!(web.hits().is_empty(), "a lookup went out before the person allowed it: {:?}", web.hits());
    let item = ask["id"].as_str().unwrap();
    // Allowing web lookups is for this task only, never longer.
    for scope in ["window", "standing"] {
        assert_eq!(alice.respond(item, json!({ "decision": "approve", "scope": scope })).await.as_u16(), 400, "{scope} was accepted");
    }
    assert!(alice.respond(item, json!({ "decision": "approve", "scope": "task" })).await.is_success());
    let t = alice.wait_status(&id, &["completed", "failed"]).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(web.hits(), ["/widgets", "/bolts"]);
    let asks: Vec<Value> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "approval").collect();
    assert_eq!(asks.len(), 1, "it asked more than once: {asks:?}");
    // The approval covers this task only: another task in the project asks again.
    let (_, v2) = alice.post("/v1/tasks", json!({ "text": "Check them again.", "project": pid })).await;
    let id2 = v2["task_id"].as_str().unwrap().to_string();
    alice.wait_item(&id2, "approval").await;
}

/// In chat the same rule asks once per conversation. A question that read
/// nothing private never asks (the owner's request 1).
#[tokio::test(flavor = "multi_thread")]
async fn chat_asks_once_per_conversation_for_web_lookups_after_an_upload_and_never_for_a_plain_question() {
    let web = FakeWeb::start(&[("/rates", "Plumber: 90 EUR an hour"), ("/llc", "An LLC is a legal structure.")]).await;
    let (rates, llc) = (web.url("/rates"), web.url("/llc"));
    let m = FakeModel::start(script(move |c| {
        let (rates, llc) = (rates.clone(), llc.clone());
        async move {
            let plain = c.messages()[0].to_string().contains("LLC");
            match (plain, c.turn()) {
                (true, 0) => tool("web_fetch", json!({ "url": llc })),
                (false, 0) => tool("web_fetch", json!({ "url": rates })),
                _ => Reply::Text("Here it is.".into()),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let file = upload(&alice, "quote.txt", "text/plain", "Plumber quote: 120 EUR an hour", json!({})).await;
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Is this plumber's rate fair?", "files": [file] })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let (thread, turn) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
    let ask = alice.wait_item(&turn, "approval").await;
    assert_eq!(ask["details"]["web_lookups"], "conversation");
    assert_eq!(ask["summary"], "This conversation reads a file you uploaded and searches the web — allow web lookups for this conversation?");
    assert!(alice.respond(ask["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "task" })).await.is_success());
    assert_eq!(alice.wait_status(&turn, &["completed", "failed"]).await["status"], "completed");
    // The file went with the message, and the next message in the thread
    // looks things up without asking again.
    let (_, th) = alice.get(&format!("/v1/chat/{thread}")).await;
    assert_eq!(th["entries"][0]["files"][0]["name"], "quote.txt", "{th}");
    let (_, v) = alice.post("/v1/chat", json!({ "text": "And in Lyon?", "thread": thread })).await;
    let turn2 = v["task_id"].as_str().unwrap().to_string();
    assert_eq!(alice.wait_status(&turn2, &["completed", "failed"]).await["status"], "completed");
    // A plain question in a new conversation read nothing private: it never asks.
    let (_, v) = alice.post("/v1/chat", json!({ "text": "What's an LLC?" })).await;
    let turn3 = v["task_id"].as_str().unwrap().to_string();
    assert_eq!(alice.wait_status(&turn3, &["completed", "failed"]).await["status"], "completed");
    let asks: Vec<Value> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "approval").collect();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(web.hits(), ["/rates", "/rates", "/llc"]);
}
