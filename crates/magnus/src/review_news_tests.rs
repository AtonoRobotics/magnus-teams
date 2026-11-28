//! A review tells the person news once (`review_news`).

use crate::admin_app::tests::server_with;
use crate::events::Ev;
use crate::state::Status;
use crate::tools_core::tests::{idle_run, POLLS, POLL_EVERY};
use axum::Router;
use chrono::Utc;
use serde_json::json;

fn queued(s: &crate::admin_app::tests::Server) -> usize {
    s.k.read(|st| st.inbox.items().values().filter(|i| i.person == "mo" && i.kind == "observation").count())
}

/// A review of mo's state, stopped where the model would act.
async fn review_run(s: &crate::admin_app::tests::Server) -> String {
    let (_, run) = s.k.create_run("review", "mo", "Review the person's state (cadence).".into(), vec![], crate::kernel::NewRun::default()).await.unwrap();
    for _ in 0..POLLS {
        if s.k.read(|st| st.runs[&run].status != Status::Running) {
            break;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
    s.k.emit("review", Ev::Resumed { run: run.clone() }).await.unwrap();
    run
}

/// The model's `observe` or `interrupt` on `run`, as the run loop records
/// it: what it was told back.
async fn act(s: &crate::admin_app::tests::Server, run: &str, n: usize, tool: &str, input: serde_json::Value) -> (String, bool) {
    let r = s.k.read(|st| st.runs[run].clone());
    let call = format!("c{n}");
    s.k.record_call(&r, &call, "s1", tool, &input).await.unwrap();
    s.k.read(|st| st.runs[run].results.get(&call).map(|r| (r.text.clone(), r.is_error))).unwrap()
}

/// Production, 2026-11-27: review after review queued the same two paused
/// schedules in other words. The second telling is refused; news that
/// names something new is not.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_news_is_queued_once_in_any_words() {
    let s = server_with("", |_| Router::new()).await;
    let run = review_run(&s).await;
    let first = json!({
        "what": "Two schedules are paused after three consecutive failures each: the daily 9:00 a.m. run and the 30-minute run.",
        "why": "Neither has a next run scheduled.",
        "evidence": "Schedules: sched_8a2d625bfcc18998e2e2769db2b05dc1 and sched_de1642f842544516a2c0932c3690e071."
    });
    let (text, _) = act(&s, &run, 0, "observe", first).await;
    assert!(text.starts_with("Queued"), "{text}");
    let again = json!({
        "what": "Both Magnus schedules—the daily run and the every-30-minute run—are paused.",
        "why": "If you still rely on either, check its latest failed run.",
        "evidence": "sched_8a2d625bfcc18998e2e2769db2b05dc; sched_de1642f842544516a2c0932c3690e071"
    });
    let (text, is_error) = act(&s, &run, 1, "observe", again).await;
    assert!(!is_error && text.starts_with("Already told"), "a dropped last character is the same id: {text}");
    let same_words = json!({ "what": "Two schedules are paused after three consecutive failures each: the daily 9:00 a.m. run and the 30-minute run.", "why": "x", "evidence": "the schedules list" });
    let (text, _) = act(&s, &run, 2, "observe", same_words).await;
    assert!(text.starts_with("Already told"), "the same words without ids: {text}");
    let (text, _) = act(
        &s,
        &run,
        3,
        "interrupt",
        json!({ "what": "Two schedules are paused after three consecutive failures each: the daily 9:00 a.m. run and the 30-minute run.", "why": "now" }),
    )
    .await;
    assert!(text.starts_with("Already told"), "an interruption is a telling too: {text}");
    assert_eq!(queued(&s), 1);
    let news = json!({
        "what": "The daily schedule failed again after it was resumed.",
        "why": "It will pause again.",
        "evidence": "sched_8a2d625bfcc18998e2e2769db2b05dc1, task_7747cb6732614914aa237820ccd00110"
    });
    let (text, _) = act(&s, &run, 4, "observe", news).await;
    assert!(text.starts_with("Queued"), "a new failure is news: {text}");
    assert_eq!(queued(&s), 2);
    let told = s.k.read(|st| crate::review_news::told_lines(st, "mo", Utc::now()));
    assert_eq!(told.len(), 2, "the review is shown both: {told:?}");
    s.done().await;
}

/// A cadence review over the state the last review read has nothing new to
/// read; what reviews told is not a change of state.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_over_the_same_state_waits_for_a_change() {
    let s = server_with("", |_| Router::new()).await;
    let now = Utc::now();
    assert!(!s.k.state_unchanged("mo", now), "a server just started knows no last review");
    s.k.note_reviewed_state("mo", now);
    assert!(s.k.state_unchanged("mo", now));
    let _task = idle_run(&s).await;
    assert!(!s.k.state_unchanged("mo", now), "a task going is a change");
    let review = review_run(&s).await;
    s.k.note_reviewed_state("mo", now);
    let (text, _) = act(&s, &review, 0, "observe", json!({ "what": "Mo's notes task is going.", "why": "w", "evidence": "e" })).await;
    assert!(text.starts_with("Queued"), "{text}");
    assert!(s.k.state_unchanged("mo", now), "what a review told is not news to the next");
    s.done().await;
}

/// What the reviews queued before they saw what they had told: each
/// older telling of the same news is dismissed at boot, the newest kept,
/// and news told once stays.
#[tokio::test(flavor = "multi_thread")]
async fn older_tellings_of_the_same_news_are_dismissed() {
    let s = server_with("", |_| Router::new()).await;
    let told = |id: &str, summary: &str| Ev::Item {
        id: id.into(),
        item_kind: "observation".into(),
        task: None,
        run: None,
        person: "mo".into(),
        summary: summary.into(),
        details: json!({ "evidence": "the schedules list" }),
    };
    for (id, summary) in [
        ("item_old", "Two schedules are paused after three consecutive failures each."),
        ("item_once", "A Robinhood deposit was reversed."),
        ("item_new", "Two schedules are paused after three consecutive failures each."),
    ] {
        s.k.emit("kernel", told(id, summary)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(s.k.dismiss_repeated_news().await.unwrap(), 1);
    let open = |id: &str| s.k.read(|st| st.inbox.items()[id].resolution.is_none());
    assert!(!open("item_old") && open("item_new") && open("item_once"));
    assert_eq!(s.k.dismiss_repeated_news().await.unwrap(), 0, "once dismissed, nothing more at the next boot");
    s.done().await;
}
