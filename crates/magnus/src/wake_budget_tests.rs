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
        if k.review_wake_items("mo", "mail", 1, true).await {
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

/// New mail arriving one message at a time wakes one review, not one each:
/// after a review of new items, more gather until the window has passed,
/// when the next read wakes one review for all of them. An urgent item
/// (a CI failure, a blocker) does not wait. Before, every new email woke a
/// review of its own: 1,032 in a week, the largest share of the tokens.
#[tokio::test(flavor = "multi_thread")]
async fn new_items_gather_into_one_review_unless_urgent() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let k = &s.k;
    let end_reviews = || async {
        let going: Vec<String> = k.read(|s| s.inbox.review_runs().iter().filter(|(_, p)| p.as_str() == "mo").map(|(r, _)| r.clone()).collect());
        for run in going {
            k.emit("kernel", Ev::Cancelled { run, source: None, turn: None }).await.unwrap();
        }
    };
    let mut started = vec![];
    for _ in 0..3 {
        started.push(k.review_wake_items("mo", "mail", 1, false).await);
        end_reviews().await;
    }
    assert_eq!(started, [true, false, false], "each new email woke its own review");
    assert!(k.review_wake_items("mo", "github", 1, true).await, "an urgent item waited for the window");
    end_reviews().await;
    let gather = crate::wake_budget::ARRIVAL_GATHER_MINUTES;
    let after = Utc::now() + Duration::minutes(i64::from(gather)) + Duration::seconds(1);
    assert!(k.read(|s| crate::wake_budget::gathering(s, "mo", Utc::now(), gather)), "the window closed at once");
    assert!(!k.read(|s| crate::wake_budget::gathering(s, "mo", after, gather)), "the window never closed: the gathered items would never be read");
    assert!(!k.read(|s| crate::wake_budget::gathering(s, "ana", Utc::now(), gather)), "one person's review held another's mail");
    s.done().await;
}
