use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Status;
use serde_json::{json, Value};

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

fn created(task: &str, run: &str, text: &str, chat: Value) -> Ev {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(json!({ "kind": "task_created", "task": task, "run": run, "person": "mo", "text": text, "config": config, "chat": chat }))
}

/// Mo's thread `th1`: a turn (`r1`, answered when `answered`) and the task
/// it started (`r2`), still going.
async fn thread(k: &Kernel, answered: bool) {
    k.emit("person:mo", created("t1", "r1", "Plan my trip to Lisbon", json!({ "thread": "th1", "turn": true }))).await.unwrap();
    k.emit("person:mo", created("t2", "r2", "Find flights to Lisbon", json!({ "thread": "th1", "turn": false, "from_run": "r1" }))).await.unwrap();
    if answered {
        k.emit("person:mo", ev(json!({ "kind": "completed", "run": "r1", "summary": "Here is your plan." }))).await.unwrap();
    }
}

fn status(k: &Kernel, run: &str) -> Status {
    k.read(|s| s.runs[run].status)
}

/// With the agent paused, or the month's spend limit reached, an edited
/// message is refused as a new message is, and nothing changes: the thread
/// shows what it showed and the work it started goes on.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_edit_leaves_the_thread_and_its_work() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    thread(&s.k, false).await;
    let mo = s.token("mo").await;
    let (_, before) = s.call(&mo, reqwest::Method::GET, "/v1/chat/th1", None).await;
    let tasks = s.k.read(|s| s.tasks.len());
    let stops: [(Ev, Ev, u16); 2] = [
        (Ev::Halt(crate::halts::Change::Paused { agent: "person:mo".into(), name: "Sam".into() }), Ev::Halt(crate::halts::Change::Unpaused { agent: "person:mo".into() }), 409),
        (
            Ev::Spend(crate::spend::Change::LimitSet { scope: crate::spend::Scope::Person("mo".into()), usd: Some(0.0) }),
            Ev::Spend(crate::spend::Change::LimitSet { scope: crate::spend::Scope::Person("mo".into()), usd: None }),
            402,
        ),
    ];
    for (stop, go, code) in stops {
        s.k.emit("person:sam", stop).await.unwrap();
        let (got, body) = s.call(&mo, reqwest::Method::POST, "/v1/chat/th1/entries/m:t1/resend", Some(json!({ "text": "Plan my trip to Porto" }))).await;
        assert_eq!(got, code, "{body}");
        assert!(body["error"].as_str().is_some_and(|e| !e.is_empty() && e != "internal error"), "{body}");
        let (_, after) = s.call(&mo, reqwest::Method::GET, "/v1/chat/th1", None).await;
        assert_eq!(after, before, "a refused edit changed the thread");
        assert_eq!(s.k.read(|s| s.tasks.len()), tasks, "a refused edit started a turn");
        for run in ["r1", "r2"] {
            assert_eq!(status(&s.k, run), Status::Running, "a refused edit stopped {run}");
        }
        s.k.emit("person:sam", go).await.unwrap();
    }
    s.done().await;
}

/// A branch carries the task ids of the thread it was made from. Retrying
/// its answer stops the branch's own work, never the original thread's.
#[tokio::test(flavor = "multi_thread")]
async fn a_retry_in_a_branch_leaves_the_original_threads_work() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    thread(&s.k, true).await;
    let mo = s.token("mo").await;
    let (code, branched) = s.call(&mo, reqwest::Method::POST, "/v1/chat/th1/entries/a:t1/branch", None).await;
    assert_eq!(code, 201, "{branched}");
    let branch = branched["thread"].as_str().unwrap().to_string();
    let (code, body) = s.call(&mo, reqwest::Method::POST, &format!("/v1/chat/{branch}/entries/a:t1/resend"), Some(json!({}))).await;
    assert_eq!(code, 201, "{body}");
    assert_eq!(status(&s.k, "r2"), Status::Running, "a retry in a branch stopped the original thread's task");
    let (_, original) = s.call(&mo, reqwest::Method::GET, "/v1/chat/th1", None).await;
    assert!(original["entries"].as_array().unwrap().iter().any(|e| e["id"] == "a:t1"), "the retry hid the original's answer: {original}");
    s.done().await;
}
