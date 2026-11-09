//! A watch with an interval polls on schedule (#406): without
//! anyone reading the app, the sweep starts a minimal polling run
//! per due watch, and new items wake a review naming the count.
//! Polls are quiet: their tasks never reach the person's list.

use magnus_invariants::*;
use magnus_scratch::{control, reconfigure};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";
const MEMORY_PASS: &str = "long-term memory";

fn done(summary: &str) -> Reply {
    tool("task_complete", json!({ "summary": summary, "deliverable": [{ "type": "text", "text": summary }] }))
}

async fn drain_reviews(bob: &Api) {
    eventually("all reviews settled", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
}

fn seen(prompts: &Arc<Mutex<Vec<String>>>, needle: &str) -> bool {
    prompts.lock().unwrap().iter().any(|p| p.contains(needle))
}

/// The world holds one message; a task read guarantees the seed.
/// Then a second message arrives and nobody reads again: the next
/// scheduled poll wakes a review naming it, and no poll task shows
/// in the person's list.
#[tokio::test(flavor = "multi_thread")]
async fn a_watch_with_an_interval_polls_without_reads() {
    let mail = mcp::mail().start().await;
    mail.set_world("inbox", json!([{ "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" }]));
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_prompts = prompts.clone();
    let m = FakeModel::start(script(move |c| {
        let seen_prompts = seen_prompts.clone();
        async move {
            if c.body["system"].to_string().contains(REVIEW) {
                seen_prompts.lock().unwrap().push(c.text());
                return done("Reviewed quietly.");
            }
            if c.turn() == 0 {
                return tool("mail__list_inbox", json!({}));
            }
            // Failing ends the task without a contract; the read it
            // made is what matters. Polls never wake on ending.
            return tool("task_fail", json!({ "reason": "checked the inbox" }));
        }
    }))
    .await;
    let app = Appliance::start(
        Spec::default().proactive(json!({ "on": true })).app("mail", &mail, "public", "private", &[("list_inbox", "read")]).watch_poll(
            "mail",
            "list_inbox",
            "/messages",
            "subject",
            1,
        ),
        &m,
    )
    .await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let first = bob.submit("Check the inbox.").await;
    assert_eq!(bob.wait_settled(&first).await["status"], "failed");
    drain_reviews(&bob).await;
    assert!(!seen(&prompts, "new in mail"), "the seeding read woke a review");
    // A message arrives; nobody reads again. The next poll wakes.
    mail.set_world(
        "inbox",
        json!([
            { "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" },
            { "from": "boss@corp.example", "subject": "First", "body": "Read this now." },
        ]),
    );
    eventually("the poll woke a review", Duration::from_secs(150), || {
        let prompts = prompts.clone();
        async move { seen(&prompts, "1 new in mail").then_some(()) }
    })
    .await;
    let (_, v) = bob.get("/v1/tasks").await;
    let polls: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["poll"] == true).collect();
    assert!(polls.is_empty(), "poll tasks reached the person's list");
}

/// A watch the person cannot use starts no poll (#418): one app
/// whose secret is missing (never connected) and one turned off by
/// an admin stay quiet, while the connected watch polls.
#[tokio::test(flavor = "multi_thread")]
async fn watches_the_person_cannot_use_are_not_polled() {
    let mail = mcp::mail().start().await;
    let cal = mcp::mail().start().await;
    let cal2 = mcp::mail().start().await;
    let polled = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_polls = polled.clone();
    let m = FakeModel::start(script(move |c| {
        let seen_polls = seen_polls.clone();
        async move {
            if c.body["system"].to_string().contains(REVIEW) {
                return done("Reviewed quietly.");
            }
            let text = c.text();
            if text.contains("Poll ") {
                let which = ["mail", "cal2", "cal"].into_iter().find(|a| text.contains(&format!("Poll {a}:"))).unwrap_or("?");
                seen_polls.lock().unwrap().push(which.to_string());
                // A poll whose app is unusable has no tool to call;
                // end without reading, as its model would.
                return tool("task_fail", json!({ "reason": "no such tool" }));
            }
            return tool("task_fail", json!({ "reason": "no tasks in this test" }));
        }
    }))
    .await;
    // Boot with the loop off, so nothing polls before cal2 is off.
    let mut app = Appliance::start(
        Spec::default()
            .app("mail", &mail, "public", "private", &[("list_inbox", "read")])
            .watch_poll("mail", "list_inbox", "/messages", "subject", 1)
            .app("cal", &cal, "public", "private", &[("list_inbox", "read")])
            .watch_poll("cal", "list_inbox", "/messages", "subject", 1)
            .without_secret("cal")
            .app("cal2", &cal2, "public", "private", &[("list_inbox", "read")])
            .watch_poll("cal2", "list_inbox", "/messages", "subject", 1),
        &m,
    )
    .await;
    let (st, v) = control(&app, "PATCH", "/v1/admin/connectors/cal2", json!({ "actor": "admin:root", "enabled": false })).await;
    assert_eq!(st, 200, "{v}");
    reconfigure(&mut app, |cfg| {
        cfg["proactive"].as_table_mut().unwrap().insert("on".into(), toml::Value::Boolean(true));
    })
    .await;
    // Past the first due sweep: mail polls, the others never do.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let polls = polled.lock().unwrap();
    assert!(polls.iter().any(|t| t == "mail"), "the connected watch never polled: {polls:?}");
    assert!(!polls.iter().any(|t| t == "cal"), "the unconnected watch polled: {polls:?}");
    assert!(!polls.iter().any(|t| t == "cal2"), "the turned-off watch polled: {polls:?}");
}

/// A poll whose read fails transiently retries and still ends
/// quietly (#419): headroom for one retry, no budget notice in
/// the inbox.
#[tokio::test(flavor = "multi_thread")]
async fn a_poll_with_a_transient_read_error_retries_quietly() {
    let fail_next = Arc::new(AtomicUsize::new(0));
    let fail_in = fail_next.clone();
    let mail = FakeMcp::builder()
        .world("inbox", json!([{ "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" }]))
        .honor_idempotency(true)
        .tool("list_inbox", "List messages in the inbox.", json!({ "type": "object", "properties": {} }), move |w, _, _| {
            // Exactly one error per arming: swap, never fetch_sub past zero.
            if fail_in.swap(0, Ordering::SeqCst) > 0 {
                mcp::err("the server hiccuped")
            } else {
                mcp::ok(json!({ "messages": w["inbox"] }))
            }
        })
        .start()
        .await;
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_prompts = prompts.clone();
    let m = FakeModel::start(script(move |c| {
        let seen_prompts = seen_prompts.clone();
        async move {
            if c.body["system"].to_string().contains(REVIEW) {
                seen_prompts.lock().unwrap().push(c.text());
                return done("Reviewed quietly.");
            }
            if c.text().contains("Poll ") {
                let read_ok = c.last_results().iter().any(|r| !r.is_error);
                if !read_ok && c.turn() < 4 {
                    return tool("mail__list_inbox", json!({}));
                }
                return done("Polled.");
            }
            if c.turn() == 0 {
                return tool("mail__list_inbox", json!({}));
            }
            return tool("task_fail", json!({ "reason": "checked the inbox" }));
        }
    }))
    .await;
    let app = Appliance::start(
        Spec::default().proactive(json!({ "on": true })).app("mail", &mail, "public", "private", &[("list_inbox", "read")]).watch_poll(
            "mail",
            "list_inbox",
            "/messages",
            "subject",
            1,
        ),
        &m,
    )
    .await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let first = bob.submit("Check the inbox.").await;
    assert_eq!(bob.wait_settled(&first).await["status"], "failed");
    drain_reviews(&bob).await;
    assert!(!seen(&prompts, "new in mail"), "the seeding read woke a review");
    // The next read fails once; a message arrives behind it. The
    // poll retries, reads, and wakes — with no budget notice.
    fail_next.store(1, Ordering::SeqCst);
    mail.set_world(
        "inbox",
        json!([
            { "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" },
            { "from": "boss@corp.example", "subject": "First", "body": "Read this now." },
        ]),
    );
    eventually("the poll retried and woke a review", Duration::from_secs(150), || {
        let prompts = prompts.clone();
        async move { seen(&prompts, "1 new in mail").then_some(()) }
    })
    .await;
    let (_, v) = bob.get("/v1/inbox").await;
    let budget: Vec<_> = v["items"].as_array().cloned().unwrap_or_default().into_iter().filter(|i| i["kind"] == "notice" && i["details"]["reason"] == "budget").collect();
    assert!(budget.is_empty(), "the retrying poll suspended on budget: {budget:?}");
}

/// A task read postpones the next poll (#413): any watched answer
/// restarts the poll clock, so a freshly-read watch is not polled
/// on the old schedule. Timed from the first poll: after reads by
/// everyone fifteen seconds in, nothing polls in the old window.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_read_postpones_the_next_poll() {
    let mail = mcp::mail().start().await;
    let calls = Arc::new(Mutex::new(Vec::<tokio::time::Instant>::new()));
    let seen_calls = calls.clone();
    let m = FakeModel::start(script(move |c| {
        let seen_calls = seen_calls.clone();
        async move {
            if c.body["system"].to_string().contains(REVIEW) {
                return done("Reviewed quietly.");
            }
            if c.text().contains("Poll ") {
                seen_calls.lock().unwrap().push(tokio::time::Instant::now());
                return tool("task_fail", json!({ "reason": "not reading in this test" }));
            }
            if c.turn() == 0 {
                return tool("mail__list_inbox", json!({}));
            }
            return tool("task_fail", json!({ "reason": "checked the inbox" }));
        }
    }))
    .await;
    let app = Appliance::start(
        Spec::default().proactive(json!({ "on": true })).app("mail", &mail, "public", "private", &[("list_inbox", "read")]).watch_poll(
            "mail",
            "list_inbox",
            "/messages",
            "subject",
            1,
        ),
        &m,
    )
    .await;
    eventually("the first poll", Duration::from_secs(30), || {
        let calls = calls.clone();
        async move { (!calls.lock().unwrap().is_empty()).then_some(()) }
    })
    .await;
    let t0 = calls.lock().unwrap().iter().min().cloned().expect("a first poll");
    // Fifteen seconds after the first poll, everyone reads: each
    // read restarts that person's poll clock.
    tokio::time::sleep_until(t0 + Duration::from_secs(15)).await;
    for who in ["alice", "bob", "carol"] {
        let api = app.api(who);
        let t = api.submit("Check the inbox.").await;
        assert_eq!(api.wait_settled(&t).await["status"], "failed");
        drain_reviews(&api).await;
    }
    // The old schedule would poll a minute after the first poll;
    // the reads postponed every clock past the window.
    tokio::time::sleep_until(t0 + Duration::from_secs(70)).await;
    let calls = calls.lock().unwrap();
    let in_window: Vec<_> = calls.iter().filter(|t| **t > t0 + Duration::from_secs(55) && **t < t0 + Duration::from_secs(70)).collect();
    assert!(in_window.is_empty(), "a poll fired on a freshly-read watch: {in_window:?}");
}

/// A due watch polls exactly once (#425): one connected watch, three
/// people, twenty seconds inside a one-minute interval — three poll
/// runs and three marks, one per person. Only agent turns count:
/// the memory pass also sees the Poll text (the six sightings that
/// filed this issue were three polls plus three memory passes). One
/// sweep loop and one kernel per log mean two sweeps cannot start
/// two polls; this pins it.
#[tokio::test(flavor = "multi_thread")]
async fn a_due_watch_polls_exactly_once() {
    let mail = mcp::mail().start().await;
    let polls = Arc::new(Mutex::new(0_usize));
    let seen_polls = polls.clone();
    let m = FakeModel::start(script(move |c| {
        let seen_polls = seen_polls.clone();
        async move {
            let system = c.body["system"].to_string();
            if system.contains(REVIEW) {
                return done("Reviewed quietly.");
            }
            if c.text().contains("Poll ") && !system.contains(MEMORY_PASS) {
                *seen_polls.lock().unwrap() += 1;
                return tool("task_fail", json!({ "reason": "not reading in this test" }));
            }
            return tool("task_fail", json!({ "reason": "no tasks in this test" }));
        }
    }))
    .await;
    // Boot with the loop off, so the first polls come from due sweeps.
    let mut app =
        Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]).watch_poll("mail", "list_inbox", "/messages", "subject", 1), &m).await;
    reconfigure(&mut app, |cfg| {
        cfg["proactive"].as_table_mut().unwrap().insert("on".into(), toml::Value::Boolean(true));
    })
    .await;
    // Past the first due sweeps, inside the one-minute interval: each
    // of the three people's watch polled exactly once, and marked.
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(*polls.lock().unwrap(), 3, "a due watch did not poll exactly once per person");
    let db = app.db.admin().await;
    let marks: i64 = db.query_one("SELECT COUNT(*) FROM public.events WHERE body->>'kind' = 'watch_polled'", &[]).await.unwrap().get(0);
    assert_eq!(marks, 3, "a poll did not mark its watch");
}
