//! No silent failures (K24): every way a run can end badly leaves a record
//! the person sees. A task the model gives up on posts an inbox notice with
//! the evidence; a chat turn whose requested schedule or task never came
//! about posts one too; every schedule firing shows in the Schedules view
//! with its task's state. And the bug that started it: a model that must
//! fill every field sends `daily_at: ""` beside `every_minutes`, which was
//! refused as both.

use magnus_invariants::*;
use serde_json::{json, Value};

const CHAT: &str = "talking with the person you work for in a chat";

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn schedule(every: u64, daily: &str) -> Reply {
    tool("create_schedule", json!({ "title": "System test", "text": "Run a system test.", "scope": "Posts a short confirmation.", "every_minutes": every, "daily_at": daily }))
}

async fn notices(api: &Api) -> Vec<Value> {
    api.get("/v1/inbox").await.1["items"].as_array().cloned().unwrap_or_default().into_iter().filter(|i| i["kind"] == "notice").collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_reach_the_inbox_and_every_schedule_firing_shows() {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            let s = said(&c);
            return match c.turn() {
                // The trace: every_minutes with an empty daily_at.
                0 if s.contains("every 5 minutes") => schedule(5, ""),
                // Too often: refused each time, and the model gives up.
                0 | 1 if s.contains("every minute") => schedule(1, ""),
                _ => Reply::Text("Done.".into()),
            };
        }
        // Tasks, scheduled or not, give up with a reason.
        tool("task_fail", json!({ "reason": "the report site was down (HTTP 503)" }))
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");

    // The empty field is no longer read as a second timing.
    let (_, v) = alice.post("/v1/chat", json!({ "text": "Schedule a system test every 5 minutes." })).await;
    let done = alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let card = m.requests().iter().flat_map(|c| c.messages()).filter_map(|msg| msg.to_string().contains("A card for this schedule").then_some(())).count();
    assert!(card > 0, "the schedule was refused: {done}");

    // A requested schedule that never came about is in the inbox.
    let (_, v) = alice.post("/v1/chat", json!({ "text": "Schedule a system test every minute." })).await;
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let n = eventually("a notice for the schedule that was not made", PATIENCE, || async {
        notices(&alice).await.into_iter().find(|i| i["summary"].as_str().unwrap_or_default().starts_with("Not done"))
    })
    .await;
    assert!(n["details"]["evidence"].to_string().contains("at most every 5 minutes"), "no evidence: {n}");

    // A task the model fails is in the inbox, with its reason.
    let t = alice.submit("Download this month's report.").await;
    alice.wait_settled(&t).await;
    let n = eventually("a notice for the failed task", PATIENCE, || async {
        notices(&alice).await.into_iter().find(|i| i["summary"].as_str().unwrap_or_default().starts_with("Failed") && i["task_id"] == json!(t))
    })
    .await;
    assert!(n["details"]["evidence"]["reason"].as_str().unwrap().contains("HTTP 503"), "{n}");

    // A schedule due in a minute fires; the Schedules view shows the firing and its result, and the failure is in the inbox.
    let at = (chrono::Local::now() + chrono::Duration::minutes(1)).format("%H:%M").to_string();
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Check the report site.", "daily_at": at })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let s = eventually("the schedule fires and its task ends", std::time::Duration::from_secs(150), || async {
        let (_, v) = alice.get("/v1/schedules").await;
        v["schedules"].as_array().unwrap().iter().find(|s| s["id"] == json!(id) && s["last"]["status"] == "failed").cloned()
    })
    .await;
    assert!(!s["last_fired"].is_null(), "{s}");
    let task = s["last"]["task_id"].clone();
    eventually("a notice for the scheduled task's failure", PATIENCE, || async { notices(&alice).await.into_iter().find(|i| i["task_id"] == task) }).await;
}

/// Each notice in the log with the event just before it: (that event's
/// kind, its actor, whether both have one time, the notice's actor). One
/// statement gives its rows one time, so the same time means one write.
async fn notices_with_what_they_tell(app: &Appliance) -> Vec<(String, String, bool, String)> {
    let rows = app.db.admin().await.query("SELECT kind, actor, at::text, body->>'item_kind' FROM public.events ORDER BY seq", &[]).await.unwrap();
    let rows: Vec<(String, String, String, Option<String>)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect();
    rows.windows(2).filter(|w| w[1].0 == "item" && w[1].3.as_deref() == Some("notice")).map(|w| (w[0].0.clone(), w[0].1.clone(), w[0].2 == w[1].2, w[1].1.clone())).collect()
}

/// A failed task's notice, and a chat turn's "Not done", are written with
/// the failure and the turn's end, in one write: nobody sees the run ended
/// without its notice, and no crash between the two leaves it untold (K24).
#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_ends_badly_and_its_notice_are_one_write() {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            return match c.turn() {
                0 | 1 => schedule(1, ""),
                _ => Reply::Text("Done.".into()),
            };
        }
        tool("task_fail", json!({ "reason": "the report site was down (HTTP 503)" }))
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let t = alice.submit("Download this month's report.").await;
    assert_eq!(alice.wait_settled(&t).await["status"], "failed");
    let (_, v) = alice.post("/v1/chat", json!({ "text": "Schedule a system test every minute." })).await;
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;

    let told = notices_with_what_they_tell(&app).await;
    assert_eq!(
        told,
        [("failed".to_string(), "model".to_string(), true, "kernel".to_string()), ("completed".into(), "kernel".into(), true, "kernel".into())],
        "each notice right after what it tells of, in the same write"
    );
}
