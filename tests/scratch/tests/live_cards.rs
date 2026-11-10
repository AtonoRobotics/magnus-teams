//! Tasks and schedules as live cards in their conversation (behavior spec,
//! Tasks and schedules in conversation): a task's card has Cancel while it
//! runs or waits; a confirmed schedule's card shows its next run and latest
//! result, with Pause, Resume and Cancel, acting directly and not through
//! the model; a result that arrives after the person has moved on quotes
//! what they asked. `desktop/tests/live-cards.mjs` drives the cards in a
//! real browser.

use magnus_invariants::appliance::workspace_root;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

const CHAT: &str = "talking with the person you work for in a chat";
const PLAN: &str = "Draft the plan for the offsite.";
const MEMO: &str = "Draft the memo about parking.";

#[tokio::test(flavor = "multi_thread")]
async fn tasks_and_schedules_are_live_cards_in_their_conversation() {
    let until = (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
    let m = FakeModel::start(script(move |c| {
        let until = until.clone();
        async move {
            let body = c.body.to_string();
            if c.body["system"].to_string().contains(CHAT) {
                let first = c.messages().first().cloned().unwrap_or_default();
                let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
                return match (said.as_str(), c.turn()) {
                    (s, 0) if s.contains("offsite") => tool("create_task", json!({ "title": "Offsite plan", "goal": PLAN })),
                    (s, 0) if s.contains("parking") => tool("create_task", json!({ "title": "Parking memo", "goal": MEMO })),
                    (s, 0) if s.contains("plants") => {
                        tool("create_schedule", json!({ "title": "Water the plants", "goal": "Remind me to water the plants.", "daily_at": "07:30" }))
                    }
                    (s, 0) if s.contains("stars") => {
                        tool("create_schedule", json!({ "title": "Count the stars", "goal": "Count the stars, once.", "every_minutes": 60, "until": until.clone() }))
                    }
                    _ => Reply::Text("On it.".into()),
                };
            }
            if body.contains(PLAN) {
                return match c.turn() {
                    0 => tool("ask_principal", json!({ "question": "Which weekend?" })),
                    _ => tool("task_fail", json!({ "reason": "The venue list is not in yet." })),
                };
            }
            tool("ask_principal", json!({ "question": "Which lot?" }))
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let say = |text: &'static str, thread: Option<String>| {
        let alice = &alice;
        async move {
            let mut b = json!({ "text": text });
            if let Some(t) = thread {
                b["thread"] = json!(t);
            }
            let (st, v) = alice.post("/v1/chat", b).await;
            assert_eq!(st.as_u16(), 201, "{v}");
            alice.wait_settled(v["task_id"].as_str().unwrap()).await;
            v["thread"].as_str().unwrap().to_string()
        }
    };
    let thread = say("Plan the offsite.", None).await;
    say("Draft a memo about parking.", Some(thread.clone())).await;
    say("Every morning, remind me to water the plants.", Some(thread.clone())).await;
    let find = |text: &'static str| {
        let alice = &alice;
        async move {
            eventually("the task", PATIENCE, || async { alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == text).cloned() }).await["task_id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };
    let (plan, memo) = (find(PLAN).await, find(MEMO).await);
    let question = alice.wait_item(&plan, "question").await;
    alice.wait_item(&memo, "question").await;
    // The schedule was set up when she asked; its card is live.
    let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
    let card = c["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "schedule_card").cloned().expect("no schedule card");
    assert_eq!(card["state"], "confirmed", "the schedule waits on a confirmation: {card}");
    // A second schedule, ending soon: set up, then over before the
    // browser opens. Its card says it ended, with why (#264).
    say("Every hour, count the stars.", Some(thread.clone())).await;
    let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
    let stars = c["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "schedule_card").cloned().collect::<Vec<_>>();
    let stars = stars.iter().find(|e| e["card"] != card["card"]).cloned().expect("no stars schedule card");
    let ended = stars["schedule"].as_str().unwrap_or_else(|| panic!("no schedule set up: {stars}")).to_string();
    eventually("the stars schedule to end", std::time::Duration::from_secs(150), || async {
        let (_, s) = alice.get("/v1/schedules").await;
        s["schedules"].as_array().unwrap().iter().find(|x| x["id"] == ended && !x["ended"].is_null()).map(|_| ())
    })
    .await;
    // The person moves on, then answers the plan's question: its result
    // comes after their newer message.
    say("Thanks. Anything else today?", Some(thread.clone())).await;
    assert!(alice.respond(question["id"].as_str().unwrap(), json!({ "decision": "answer", "text": "The first weekend of June." })).await.is_success());
    assert_eq!(alice.wait_settled(&plan).await["status"], "failed");

    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/live-cards.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_THREAD", &thread)
        .env("MAGNUS_MEMO", &memo)
        .env("MAGNUS_PLAN", &plan)
        .env("MAGNUS_ENDED", &ended)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the live cards failed:\n{log}");
    // What the buttons did, in the appliance itself.
    assert_eq!(alice.wait_settled(&memo).await["status"], "cancelled", "Cancel on the task's card did not cancel it");
    let (_, s) = alice.get("/v1/schedules").await;
    let left = s["schedules"].as_array().unwrap().clone();
    assert_eq!(left.len(), 1, "Cancel on the schedule's card did not cancel it: {s}");
    assert_eq!(left[0]["id"], ended, "the wrong schedule is left: {s}");
    assert!(!left[0]["ended"].is_null(), "the ended schedule lost its mark: {s}");
}
