use crate::events::Ev;
use crate::state::State;
use chrono::{Duration, Utc};

/// Eleven wakes in an hour start ten reviews, and a restart (the state
/// replayed from the log) still counts the ten: the eleventh stays refused
/// until the hour has passed.
#[tokio::test(flavor = "multi_thread")]
async fn eleven_wakes_in_an_hour_start_ten_reviews_and_a_restart_keeps_the_count() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let k = &s.k;
    let mut started = 0;
    for _ in 0..11 {
        if k.review_wake_items("mo", "mail", 1).await {
            started += 1;
        }
        // Each review ends before the next mail, so no review going holds the next.
        let going: Vec<String> = k.read(|s| s.inbox.review_runs().iter().filter(|(_, p)| p.as_str() == "mo").map(|(r, _)| r.clone()).collect());
        for run in going {
            k.emit("kernel", Ev::Cancelled { run, source: None, turn: None }).await.unwrap();
        }
    }
    assert_eq!(started, 10, "eleven wakes in an hour started {started} reviews");
    assert_eq!(k.read(|s| s.tasks.values().filter(|t| t.review && t.person == "mo").count()), 10);

    let log = crate::log::Log::read_only(&k.cfg.database.url).await.unwrap();
    let mut replayed = State::default();
    for e in log.load().await.unwrap() {
        replayed.apply(&e);
    }
    let now = Utc::now();
    assert!(crate::wake_budget::refused(&replayed, "mo", now).is_some(), "a restart forgot the wakes");
    assert!(crate::wake_budget::refused(&replayed, "ana", now).is_none(), "one person's wakes counted against another");
    let later = now + Duration::seconds(crate::wake_budget::HOUR_SECS) + Duration::seconds(1);
    assert!(crate::wake_budget::refused(&replayed, "mo", later).is_none(), "the hour passed and the budget did not come back");
    s.done().await;
}

/// The day's budget holds across hours: fifty wakes spread over a day
/// refuse the next one though no hour is full.
#[test]
fn a_day_of_wakes_refuses_the_next_though_no_hour_is_full() {
    let mut s = State::default();
    let start = Utc::now() - Duration::hours(20);
    for i in 0..crate::wake_budget::MAX_WAKES_PER_DAY {
        let at = start + Duration::minutes(20 * i as i64);
        let ev = Ev::ReviewWoken { run: format!("r{i}"), task: format!("t{i}"), person: "mo".into(), reason: "cadence".into() };
        s.apply(&crate::log::Event { seq: i as i64 + 1, at, actor: "kernel".into(), ev });
    }
    let now = start + Duration::minutes(20 * crate::wake_budget::MAX_WAKES_PER_DAY as i64);
    assert!(crate::wake_budget::refused(&s, "mo", now).is_some_and(|why| why.contains("day")), "the day's fifty did not refuse the next");
}
