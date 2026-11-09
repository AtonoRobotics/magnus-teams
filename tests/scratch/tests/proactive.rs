//! The proactive loop (behavior step 7, #237): Magnus reviews each
//! person's state on its own, stays quiet when there is nothing to say,
//! and otherwise leaves an observation for their next session or, rarely,
//! interrupts them. Observations wait through quiet hours; interruptions
//! outside working hours or over the daily budget queue as observations
//! instead. The `[proactive]` kill switch stops all of it.
//!
//! The sweep fires before any test can set preferences, so each test
//! drives a review it triggers itself: preferences first, then a schedule
//! due in five minutes, whose coming-up review runs the script. Natural
//! cadence and overnight reviews finish quietly.

use chrono::Timelike;
use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::reconfigure;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";

fn done(summary: &str) -> Reply {
    tool("task_complete", json!({ "summary": summary, "deliverable": [{ "type": "text", "text": "Reviewed." }] }))
}

/// Natural reviews finish quietly; the coming-up review the test
/// triggered calls the scripted tools in order, then finishes. Anything
/// else fails fast: no other run is under test.
fn model(calls: Arc<Mutex<Vec<(String, Value)>>>) -> Script {
    script(move |c| {
        let calls = calls.clone();
        async move {
            if !c.body["system"].to_string().contains(REVIEW) {
                return tool("task_fail", json!({ "reason": "not under test" }));
            }
            let first = c.messages().first().cloned().unwrap_or_default();
            let said: String = first["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
            if !said.contains("coming up") {
                return done("Reviewed quietly.");
            }
            match calls.lock().unwrap().get(c.turn()).cloned() {
                Some((t, input)) => tool(&t, input),
                None => done("Reviewed; nothing further."),
            }
        }
    })
}

async fn prefer(api: &Api, body: Value) {
    let (st, v) = api.send(reqwest::Method::PUT, "/v1/preferences", Some(body)).await;
    assert!(st.is_success(), "{st} {v}");
}

/// Set preferences, then a schedule due in five minutes: its coming-up
/// review starts after both land, so it always sees these preferences.
async fn trigger_review(api: &Api, prefs: Value) {
    prefer(api, prefs).await;
    let (st, v) = api.post("/v1/schedules", json!({ "text": "Check the backups.", "every_minutes": 5 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
}

/// A one-hour window starting two hours from now, UTC: it never contains
/// the test's own run, whatever time it is.
fn away_from_now() -> Value {
    let h = (chrono::Utc::now().hour() as i64 + 2).rem_euclid(24);
    json!({ "start": format!("{h:02}:00"), "end": format!("{:02}:01", (h + 1).rem_euclid(24)) })
}

/// At least five hours of headroom, even if an hour boundary or midnight
/// passes while the real model/worker processes the triggered review.
fn quiet_around(minute: u32) -> Value {
    let hhmm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
    json!({ "start": hhmm((minute + 1440 - 360) % 1440), "end": hhmm((minute + 360) % 1440) })
}

#[test]
fn all_day_and_quiet_fixtures_cover_every_minute_and_a_delayed_review() {
    let cfg = magnus::config::Proactive::default();
    for minute in 0..1440 {
        let prefs = json!({"working_hours":{"start":"00:00","end":"00:00"},"quiet_hours":quiet_around(minute)});
        let settings = magnus::proactive::settings(&cfg, &prefs);
        for delay in [0, 1, 2, 5, 59] {
            let later = (minute + delay) % 1440;
            assert!(magnus::proactive::in_window(later, settings.working), "work {minute}+{delay}");
            assert!(magnus::proactive::in_window(later, settings.quiet), "quiet {minute}+{delay}");
        }
    }
}

async fn wait_open(api: &Api, kind: &str) -> Value {
    let me = api.clone();
    eventually(&format!("open {kind} item"), Duration::from_secs(60), || {
        let me = me.clone();
        async move { me.inbox().await.into_iter().find(|i| i["kind"] == kind && i["status"] == "open") }
    })
    .await
}

fn notify_at(item: &Value) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(item["details"]["notify_at"].as_str().unwrap()).unwrap().into()
}

#[tokio::test(flavor = "multi_thread")]
async fn review_observes_and_the_person_sees_it() {
    let calls =
        Arc::new(Mutex::new(vec![("observe".to_string(), json!({ "what": "The backup finished late", "why": "It may overlap the morning run", "evidence": "schedule backup" }))]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    // Working all day, never quiet now: the observation pings at once.
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    let item = wait_open(&bob, "observation").await;
    assert_eq!(item["summary"], "The backup finished late");
    assert_eq!(item["details"]["why"], "It may overlap the morning run");
    assert_eq!(item["details"]["evidence"], "schedule backup");
    assert!(notify_at(&item) <= chrono::Utc::now() + chrono::Duration::minutes(2), "it should ping at once: {item}");

    // The review itself ran to completion, marked as a review.
    let me = bob.clone();
    let task = eventually("completed review task", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            v["tasks"].as_array().cloned().unwrap_or_default().into_iter().find(|t| t["review"] == true && t["status"] == "completed")
        }
    })
    .await;
    assert_eq!(task["person"], "bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupt_outside_working_hours_queues_until_morning() {
    let calls = Arc::new(Mutex::new(vec![("interrupt".to_string(), json!({ "what": "The deploy failed", "why": "The site is down" }))]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": away_from_now() })).await;

    let item = wait_open(&bob, "observation").await;
    assert_eq!(item["summary"], "The deploy failed");
    assert!(item["details"]["evidence"].as_str().unwrap().contains("outside working hours"), "{item}");
    assert!(notify_at(&item) > chrono::Utc::now(), "it should wait for working hours: {item}");

    assert!(bob.inbox().await.into_iter().all(|i| !(i["kind"] == "interrupt" && i["status"] == "open")), "an interruption landed outside working hours");
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupt_in_quiet_hours_posts_now_and_pings_later() {
    let calls = Arc::new(Mutex::new(vec![("interrupt".to_string(), json!({ "what": "The deploy failed", "why": "The site is down" }))]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    // Equal working endpoints cover the complete day, including 23:59.
    let now = chrono::Utc::now();
    let quiet = quiet_around(now.hour() * 60 + now.minute());
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": quiet })).await;

    let item = wait_open(&bob, "interrupt").await;
    assert_eq!(item["summary"], "The deploy failed");
    assert!(notify_at(&item) > chrono::Utc::now(), "it should ping when quiet ends: {item}");
}

#[tokio::test(flavor = "multi_thread")]
async fn second_interrupt_same_day_queues_when_budget_spent() {
    let calls = Arc::new(Mutex::new(vec![
        ("interrupt".to_string(), json!({ "what": "First failure", "why": "It matters" })),
        ("interrupt".to_string(), json!({ "what": "Second failure", "why": "It also matters" })),
    ]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true, "interruptions_per_day": 1 })), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    let me = bob.clone();
    let both = eventually("an interruption and a queued observation", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let inbox = me.inbox().await;
            let landed = inbox.iter().find(|i| i["kind"] == "interrupt" && i["status"] == "open").cloned();
            let queued = inbox.iter().find(|i| i["kind"] == "observation" && i["status"] == "open").cloned();
            landed.zip(queued)
        }
    })
    .await;
    assert_eq!(both.0["summary"], "First failure");
    assert_eq!(both.1["summary"], "Second failure");
    assert!(both.1["details"]["evidence"].as_str().unwrap().contains("budget spent"), "{:?}", both.1);
}

/// Cadence and an event due together start one review, not two: the
/// event goes first, and the cadence waits its turn (#381).
#[tokio::test(flavor = "multi_thread")]
async fn cadence_and_event_due_together_start_one_review() {
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = prompts.clone();
    let m = FakeModel::start(script(move |c| {
        let seen = seen.clone();
        async move {
            seen.lock().unwrap().push(c.text());
            done("Reviewed quietly.")
        }
    }))
    .await;
    // Boot with the loop off: no boot reviews, so the first sweep after
    // enabling has cadence (never reviewed) and the event due together.
    let mut app = Appliance::start(Spec::default(), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" } })).await;
    reconfigure(&mut app, |cfg| {
        cfg["proactive"].as_table_mut().unwrap().insert("on".into(), toml::Value::Boolean(true));
    })
    .await;
    let bob = app.api("bob");
    // Wait for a finished review: a buggy second starts in the same sweep,
    // so it exists by then.
    eventually("a finished review", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (reviews.iter().any(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
    let (_, v) = bob.get("/v1/tasks").await;
    let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
    assert_eq!(reviews.len(), 1, "one sweep started {} reviews", reviews.len());
    assert!(prompts.lock().unwrap().iter().any(|p| p.contains("coming up")), "the event did not go first");
}

/// A failed task wakes a review that names the task and why it failed:
/// the triggering task's ID and outcome arrive with the wake, not
/// through the unfinished-work section, which excludes ended tasks (#383).
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_task_wakes_a_review_that_names_it() {
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = prompts.clone();
    let m = FakeModel::start(script(move |c| {
        let prompts = seen.clone();
        async move {
            if !c.body["system"].to_string().contains(REVIEW) {
                return tool("task_fail", json!({ "reason": "the backup drive is full" }));
            }
            prompts.lock().unwrap().push(c.text());
            done("Reviewed quietly.")
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    // Drain the boot review first: a wake during an active review is
    // dropped for the backstop to pick up (#386), so the failing task
    // waits its turn to exercise the fast wake.
    eventually("bob's boot review drained", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
    let id = bob.submit("Back up the photos.").await;
    let t = bob.wait_settled(&id).await;
    assert_eq!(t["status"], "failed", "{t}");
    eventually("a review naming the failed task", Duration::from_secs(60), || {
        let (prompts, id) = (prompts.clone(), id.clone());
        async move { prompts.lock().unwrap().iter().any(|p| p.contains(&id) && p.contains("backup drive is full")).then_some(()) }
    })
    .await;
}

/// A task that ends while a review is active is picked up by a later
/// sweep: the fast wake is dropped (#386's race), but the sweep's
/// backstop finds the newest ended task with no wake mark and reviews
/// it by name. The boot review is stalled so the failure is guaranteed
/// to land while it is active.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_ending_during_an_active_review_is_picked_up_by_the_backstop() {
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = prompts.clone();
    let m = FakeModel::start(script(move |c| {
        let prompts = seen.clone();
        async move {
            if !c.body["system"].to_string().contains(REVIEW) {
                return tool("task_fail", json!({ "reason": "the backup drive is full" }));
            }
            prompts.lock().unwrap().push(c.text());
            tokio::time::sleep(Duration::from_secs(10)).await;
            done("Reviewed quietly.")
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    // The boot review must be active when the task fails: wait for it
    // to be running, then fail immediately.
    eventually("bob's boot review running", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            v["tasks"].as_array().cloned().unwrap_or_default().into_iter().any(|t| t["review"] == true && t["status"] == "running").then_some(())
        }
    })
    .await;
    let id = bob.submit("Back up the photos.").await;
    let t = bob.wait_settled(&id).await;
    assert_eq!(t["status"], "failed", "{t}");
    eventually("the backstop review naming the failed task", Duration::from_secs(90), || {
        let (prompts, id) = (prompts.clone(), id.clone());
        async move { prompts.lock().unwrap().iter().any(|p| p.contains(&id) && p.contains("backup drive is full")).then_some(()) }
    })
    .await;
}

/// A review woken by a tainted task carries the taint from its first
/// step: the outcome it reads can quote what the task read, and must
/// not enter clean (#390).
#[tokio::test(flavor = "multi_thread")]
async fn a_review_woken_by_a_tainted_task_carries_the_taint() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return match c.turn() {
                0 => tool("mail__list_inbox", json!({})),
                _ => tool("task_fail", json!({ "reason": "saw the mail" })),
            };
        }
        done("Reviewed quietly.")
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })).app("mail", &mail, "private", "private+untrusted", &[("list_inbox", "read")]), &m).await;
    let bob = app.api("bob");
    // Drain the boot review first: a wake during an active review is
    // dropped for the backstop to pick up (#386).
    eventually("bob's boot review drained", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
    let id = bob.submit("Read the mail.").await;
    let t = bob.wait_settled(&id).await;
    assert_eq!(t["status"], "failed", "{t}");
    assert_eq!(t["taint"], json!({ "untrusted": true, "private": true }), "the failed task is not tainted: {t}");
    eventually("a tainted review", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            let mut tainted = false;
            for r in &reviews {
                let t = me.task(r["task_id"].as_str().unwrap()).await;
                tainted |= t["taint"] == json!({ "untrusted": true, "private": true });
            }
            tainted.then_some(())
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn proactive_off_runs_no_reviews() {
    let calls = Arc::new(Mutex::new(vec![("observe".to_string(), json!({ "what": "Something happened", "why": "It matters", "evidence": "somewhere" }))]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    never("any observation or interruption", Duration::from_secs(8), || {
        let (alice, bob) = (alice.clone(), bob.clone());
        async move {
            let mut all = alice.inbox().await;
            all.extend(bob.inbox().await);
            all.into_iter().any(|i| (i["kind"] == "observation" || i["kind"] == "interrupt") && i["status"] == "open")
        }
    })
    .await;
}

/// A review that writes a contract and decides nothing changes nothing in
/// the world, so it completes without asking the person to check its own
/// judgement (2026-11-07: a review that found nothing put "Check them
/// yourself: choose an appropriate response" in front of the owner).
#[tokio::test(flavor = "multi_thread")]
async fn a_review_that_decides_nothing_needs_no_review_whatever_its_contract() {
    let calls = Arc::new(Mutex::new(vec![(
        "task_contract".to_string(),
        json!({
            "goal": "Review the supplied state",
            "assumptions": [],
            "irreversible_actions": [],
            "criteria": [{ "id": "c1", "description": "Choose an appropriate response to the supplied state" }]
        }),
    )]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    // The triggered review records its contract, decides nothing, and
    // completes without waiting on the person.
    let me = bob.clone();
    eventually("the coming-up review completed", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            v["tasks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .find(|t| t["review"] == true && t["status"] == "completed" && t["text"].as_str().is_some_and(|x| x.contains("coming up")))
        }
    })
    .await;
    let me = bob.clone();
    never("a review run asked the person to review it", Duration::from_secs(3), || {
        let me = me.clone();
        async move { me.inbox().await.iter().any(|i| i["kind"] == "review") }
    })
    .await;
}

/// Background work asks no one (owner, 2026-11-07): a review whose action
/// would need the person's approval gets a refusal it can act on, and no
/// approval, review or notice reaches the person.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_that_would_need_approval_is_refused_and_asks_no_one() {
    let mail = mcp::mail().start().await;
    let calls = Arc::new(Mutex::new(vec![
        (
            "task_contract".to_string(),
            json!({ "goal": "Tell Bob the backups are late", "assumptions": [], "irreversible_actions": ["mail__send"], "criteria": [{ "id": "c1", "description": "Bob was told" }] }),
        ),
        ("mail__send".to_string(), json!({ "to": "bob@corp.example", "subject": "Backups", "body": "The backups are late." })),
    ]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })).app("mail", &mail, "public", "private", &[("send", "irreversible")]), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    let me = bob.clone();
    eventually("the coming-up review completed", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            v["tasks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .find(|t| t["review"] == true && t["status"] == "completed" && t["text"].as_str().is_some_and(|x| x.contains("coming up")))
        }
    })
    .await;
    assert_eq!(mail.effects("send"), 0, "background work sent without approval");
    assert!(m.requests().iter().any(|r| r.last_results().iter().any(|x| x.is_error && x.text.contains("background work"))), "the review was not told why it was refused");
    let me = bob.clone();
    never("background work put a card in front of the person", Duration::from_secs(3), || {
        let me = me.clone();
        async move { me.inbox().await.iter().any(|i| ["approval", "review", "notice", "question"].contains(&i["kind"].as_str().unwrap_or_default())) }
    })
    .await;
}

/// Background work that stops on its budget ends quietly, and the next
/// review still starts: a stopped review used to wait on the person with a
/// notice, and held every later review for them (owner, 2026-11-07).
#[tokio::test(flavor = "multi_thread")]
async fn a_review_stopped_by_its_budget_ends_quietly_and_reviews_go_on() {
    // The review for the first schedule coming up spends its whole budget
    // on a tool it was not given; every other review finishes at once.
    let burner: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let m = FakeModel::start(script(move |c| {
        let burner = burner.clone();
        async move {
            if !c.body["system"].to_string().contains(REVIEW) {
                return tool("task_fail", json!({ "reason": "not under test" }));
            }
            let first = c.messages().first().cloned().unwrap_or_default().to_string();
            let Some(why) = first.split("Review the person's state (").nth(1).and_then(|r| r.split(')').next()).filter(|r| r.ends_with("coming up")) else {
                return done("Reviewed quietly.");
            };
            let mut b = burner.lock().unwrap();
            if b.get_or_insert_with(|| why.to_string()) == why {
                return tool("no_such_tool", json!({}));
            }
            done("Reviewed; nothing further.")
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;
    let coming_up = |status: &'static str| {
        let me = bob.clone();
        move || {
            let me = me.clone();
            async move {
                let (_, v) = me.get("/v1/tasks").await;
                v["tasks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|t| t["review"] == true && t["status"] == status && t["text"].as_str().is_some_and(|x| x.contains("coming up")))
            }
        }
    };
    eventually("the first review ended on its budget", Duration::from_secs(90), coming_up("failed")).await;

    // A second schedule coming up wakes another review: nothing holds it.
    let (st, v) = bob.post("/v1/schedules", json!({ "text": "Check the mail server.", "every_minutes": 5 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    eventually("a later review completed", Duration::from_secs(60), coming_up("completed")).await;
    let me = bob.clone();
    never("background work put a notice in front of the person", Duration::from_secs(3), || {
        let me = me.clone();
        async move { me.inbox().await.iter().any(|i| i["kind"] == "notice" || i["kind"] == "review") }
    })
    .await;
}

/// Background cleanup puts away only a review's plumbing: an interruption
/// is one of its outputs, so it stays until the person reads it (#446
/// review: the sweep dismissed interruptions within half a second).
#[tokio::test(flavor = "multi_thread")]
async fn a_review_interruption_stays_open_after_the_sweep() {
    let calls = Arc::new(Mutex::new(vec![("interrupt".to_string(), json!({ "what": "The deploy failed", "why": "The site is down" }))]));
    let m = FakeModel::start(model(calls)).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    let item = wait_open(&bob, "interrupt").await;
    let id = item["id"].clone();
    let me = bob.clone();
    never("the sweep put away the review's interruption", Duration::from_secs(3), || {
        let (me, id) = (me.clone(), id.clone());
        async move { me.inbox().await.iter().any(|i| i["id"] == id && i["status"] != "open") }
    })
    .await;
}

/// A review proposes a task the person approves into a real task (#393):
/// the proposal lands in the inbox (not the thread), grounded in the
/// cited evidence, and accepting starts the task with the approved budget
/// while rejecting leaves no such task behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_proposes_a_task_the_person_approves() {
    let prior = Arc::new(Mutex::new(String::new()));
    let task_id = prior.clone();
    let m = FakeModel::start(script(move |c| {
        let task_id = task_id.clone();
        async move {
            if !c.body["system"].to_string().contains(REVIEW) {
                return tool("task_fail", json!({ "reason": "not under test" }));
            }
            let first = c.messages().first().cloned().unwrap_or_default();
            let said: String = first["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
            if !said.contains("coming up") {
                return done("Reviewed quietly.");
            }
            let proposal = |title: &str, text: &str| {
                tool(
                    "propose_task",
                    json!({
                        "title": title, "text": text,
                        "noticed": { "what": "stale backup index", "where": "backups app" },
                        "outcome": "a fresh backup index", "why_now": "backups overlap the morning run",
                        "needs": {
                            "permissions": ["Reads the backup index", "Rebuilds the backup index"],
                            "estimated_cost_usd": 1.5
                        },
                        "evidence": [{ "task": task_id.lock().unwrap().clone(), "shows": "bob asked about backups before" }],
                    }),
                )
            };
            match c.turn() {
                0 => proposal("Rebuild the backup index", "Rebuild the stale backup index."),
                1 => proposal("Defragment the backup index", "Defragment the stale backup index."),
                _ => done("Reviewed; nothing further."),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    *prior.lock().unwrap() = bob.submit("Check the backups.").await;
    trigger_review(&bob, json!({ "working_hours": { "start": "00:00", "end": "00:00" }, "quiet_hours": away_from_now() })).await;

    // Both proposals land in the inbox, grounded and budgeted.
    let me = bob.clone();
    let items = eventually("two task proposals", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let open: Vec<_> = me.inbox().await.into_iter().filter(|i| i["kind"] == "task_proposal" && i["status"] == "open").collect();
            (open.len() == 2).then_some(open)
        }
    })
    .await;
    for item in &items {
        let details = &item["details"];
        assert!(details["evidence"].as_array().is_some_and(|e| !e.is_empty()), "ungrounded proposal: {item}");
        assert_eq!(details["budget"]["max_usd"], 1.5, "proposal carries no budget: {item}");
    }

    // Accepting one starts the task; rejecting the other ends it quietly.
    let accept = items.iter().find(|i| i["details"]["title"] == "Rebuild the backup index").expect("the approval is missing");
    let reject = items.iter().find(|i| i["details"]["title"] == "Defragment the backup index").expect("the rejection is missing");
    assert!(bob.respond(accept["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.is_success());
    let me = bob.clone();
    let task = eventually("the accepted proposal's task", Duration::from_secs(60), || {
        let me = me.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            v["tasks"].as_array().into_iter().flatten().find(|t| t["title"] == "Rebuild the backup index").cloned()
        }
    })
    .await;
    assert!(bob.respond(reject["id"].as_str().unwrap(), json!({ "decision": "reject" })).await.is_success());
    let (_, detail) = bob.get(&format!("/v1/tasks/{}", task["task_id"].as_str().unwrap())).await;
    assert_eq!(detail["budget"]["max_usd"], 1.5, "the run lost its approved budget: {detail}");
    let rejected = bob.inbox().await.into_iter().find(|i| i["id"] == reject["id"]).expect("the rejection vanished");
    assert_eq!(rejected["status"], "resolved", "the rejection is still open: {rejected}");
    let (_, tasks) = bob.get("/v1/tasks").await;
    assert!(tasks["tasks"].as_array().into_iter().flatten().all(|t| t["title"] != "Defragment the backup index"), "the rejected proposal started anyway: {tasks}");
}
