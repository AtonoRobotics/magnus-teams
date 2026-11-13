use super::access::Access;
use super::enforce::{choose, left, left_now, ModelNotAllowed, SpendLimited};
use super::*;
use crate::log::Event;
use crate::state::State;
use chrono::TimeZone;
use serde_json::json;

fn at(d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, d, 12, 0, 0).single().unwrap()
}

fn ev(seq: i64, when: DateTime<Utc>, actor: &str, ev: serde_json::Value) -> Event {
    Event { seq, at: when, actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
}

fn spend(s: &mut State, seq: i64, c: Change) {
    s.apply(&Event { seq, at: at(10), actor: "person:admin".into(), ev: crate::events::Ev::Spend(c) });
}

/// A run of `person` started on `day` of November 2026 on `model` that
/// spent `usd`.
fn run(s: &mut State, seq: i64, person: &str, day: u32, model: &str, usd: f64) {
    let (adapter, m) = model.split_once('/').unwrap();
    let config = json!({ "adapter": adapter, "model": m, "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 50.0 }, "verifier": false, "instructions": "", "harness": "" });
    let (task, id) = (format!("t{seq}"), format!("r{seq}"));
    s.apply(&ev(seq, at(day), &format!("person:{person}"), json!({ "kind": "task_created", "task": task, "run": id, "person": person, "text": "x", "config": config })));
    s.runs.get_mut(&id).unwrap().usd = usd;
}

fn limit(scope: Scope, usd: f64) -> Change {
    Change::LimitSet { scope, usd: Some(usd) }
}

#[test]
fn a_month_is_its_calendar_month() {
    assert_eq!(month::key(at(10)), "2026-11");
    let (start, end) = month::bounds("2026-12").unwrap();
    assert_eq!((start.to_rfc3339(), end.to_rfc3339()), ("2026-12-01T00:00:00+00:00".to_string(), "2027-01-01T00:00:00+00:00".to_string()));
    assert!(month::bounds("nope").is_none());
}

#[test]
fn new_receipts_and_reservations_are_counted_once_in_the_call_month() {
    let mut s = State::default();
    run(&mut s, 1, "ada", 1, "a/main", 2.0);
    let charge = calls::Charge {
        id: "c".into(),
        run: "r1".into(),
        person: "ada".into(),
        workspace: None,
        channel: String::new(),
        model: "a/review".into(),
        month: "2026-12".into(),
        usd: 1.0,
        settled: false,
    };
    spend(&mut s, 2, Change::CallReserved { charge, baseline: Some(2.0) });
    assert_eq!(month::spent(&s, "2026-11").total_usd, 2.0);
    assert_eq!(month::spent(&s, "2026-12").total_usd, 1.0);
    spend(&mut s, 3, Change::CallSettled { id: "c".into(), usd: 0.25 });
    // The caller's normal reply still updates run.usd: it must not charge twice.
    s.runs.get_mut("r1").unwrap().usd += 0.25;
    assert_eq!(month::spent(&s, "2026-11").total_usd, 2.0);
    assert_eq!(month::spent(&s, "2026-12").by_model["a/review"], 0.25);
    assert_eq!(month::spent_by(&s, "2026-12", "ada"), (0.25, 0.25));
}

/// Limits cascade: a person's own limit, then the server's, whichever is
/// tighter; no person passes the server's.
#[test]
fn limits_cascade_so_nobody_passes_the_servers() {
    let mut l = Ledger::default();
    assert_eq!(left(&l, 500.0, 500.0, "ada", "2026-11"), Ok(None), "no limit, nothing stops");
    l.apply(at(1), &limit(Scope::Default, 100.0));
    assert_eq!(left(&l, 10.0, 10.0, "ada", "2026-11"), Ok(Some(90.0)));
    l.apply(at(1), &limit(Scope::Person("ada".into()), 300.0));
    assert_eq!(left(&l, 10.0, 10.0, "ada", "2026-11"), Ok(Some(290.0)), "an override replaces the default");
    assert_eq!(left(&l, 10.0, 10.0, "bob", "2026-11"), Ok(Some(90.0)), "everyone else keeps the default");
    l.apply(at(1), &limit(Scope::Server, 200.0));
    assert_eq!(left(&l, 150.0, 10.0, "ada", "2026-11"), Ok(Some(50.0)), "the server's limit is tighter than her own");
    let stop = left(&l, 200.0, 10.0, "ada", "2026-11").unwrap_err();
    assert_eq!((stop.which, stop.can_ask), ("server", false), "a spent server limit is the admins' to raise");
    assert!(stop.to_string().contains("The server has reached its spend limit"), "{stop}");
}

#[test]
fn a_spent_personal_limit_refuses_and_offers_to_ask() {
    let mut l = Ledger::default();
    l.apply(at(1), &limit(Scope::Default, 20.0));
    let stop = left(&l, 30.0, 20.0, "ada", "2026-11").unwrap_err();
    assert_eq!(stop, SpendLimited { which: "person", month: "2026-11".into(), spent_usd: 20.0, limit_usd: 20.0, can_ask: true, pending: false });
    assert!(stop.to_string().contains("You can ask an admin for more"), "{stop}");
}

/// Enforcement where runs are charged: the month's runs of the person and
/// of everyone are summed; last month's do not count.
#[test]
fn the_months_runs_are_what_a_start_is_checked_against() {
    let mut s = State::default();
    spend(&mut s, 1, limit(Scope::Default, 10.0));
    spend(&mut s, 2, limit(Scope::Server, 25.0));
    run(&mut s, 3, "ada", 2, "a/cheap", 6.0);
    run(&mut s, 4, "bob", 3, "a/costly", 15.0);
    assert_eq!(left_now(&s, "ada", at(10)), Ok(Some(4.0)), "her own 4 left is tighter than the server's 4");
    run(&mut s, 5, "ada", 4, "a/cheap", 4.0);
    let stop = left_now(&s, "ada", at(10)).unwrap_err();
    assert_eq!(stop.which, "server", "the server's 25 is spent before hers is");
    let next = Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 1).single().unwrap();
    assert_eq!(left_now(&s, "ada", next), Ok(Some(10.0)), "a new month starts afresh");
}

#[test]
fn spend_is_broken_down_by_person_agent_model_and_project() {
    let mut s = State::default();
    run(&mut s, 1, "ada", 2, "a/cheap", 1.5);
    run(&mut s, 2, "ada", 3, "b/costly", 2.0);
    run(&mut s, 3, "bob", 4, "a/cheap", 0.5);
    let sp = month::spent(&s, "2026-11");
    assert_eq!(sp.total_usd, 4.0);
    assert_eq!((sp.by_person["ada"], sp.by_person["bob"]), (3.5, 0.5));
    assert_eq!((sp.by_model["a/cheap"], sp.by_model["b/costly"]), (2.0, 2.0));
    assert_eq!(sp.by_agent["personal"], 4.0);
    assert_eq!(sp.by_project[""].1, 4.0, "work in no project is counted as such");
    assert_eq!(month::spent(&s, "2026-10").total_usd, 0.0);
}

/// Admins are alerted at 75% and at 90% of a limit, once each a month.
#[test]
fn alerts_come_at_75_and_90_percent_once_a_month() {
    let mut s = State::default();
    spend(&mut s, 1, limit(Scope::Server, 100.0));
    run(&mut s, 2, "ada", 2, "a/m", 70.0);
    assert!(alerts::due(&s, at(10)).is_empty(), "70% alerted");
    s.runs.get_mut("r2").unwrap().usd = 80.0;
    let due = alerts::due(&s, at(10));
    assert_eq!(due.iter().map(|d| (d.limit.as_str(), d.percent)).collect::<Vec<_>>(), vec![("server", 75)]);
    for (i, d) in due.into_iter().enumerate() {
        spend(&mut s, 10 + i as i64, Change::Alerted { limit: d.limit, month: d.month, percent: d.percent, spent_usd: d.spent_usd, limit_usd: d.limit_usd });
    }
    assert!(alerts::due(&s, at(10)).is_empty(), "75% was alerted twice");
    s.runs.get_mut("r2").unwrap().usd = 95.0;
    assert_eq!(alerts::due(&s, at(10)).iter().map(|d| d.percent).collect::<Vec<_>>(), vec![90]);
    spend(&mut s, 20, Change::Alerted { limit: "server".into(), month: "2026-11".into(), percent: 90, spent_usd: 95.0, limit_usd: 100.0 });
    assert!(alerts::due(&s, at(10)).is_empty(), "90% was alerted twice");
    // A person's own limit is watched too, with its own key.
    spend(&mut s, 21, limit(Scope::Person("ada".into()), 100.0));
    assert_eq!(alerts::due(&s, at(10)).iter().map(|d| (d.limit.as_str(), d.percent)).collect::<Vec<_>>(), vec![("person:ada", 75), ("person:ada", 90)]);
}

/// An approved request raises the person's limit for that month alone; a
/// declined one changes nothing. Each is decided once.
#[test]
fn an_increase_request_is_approved_for_the_month_or_declined() {
    let mut l = Ledger::default();
    l.apply(at(1), &limit(Scope::Default, 20.0));
    let ask = |id: &str| Change::IncreaseAsked { id: id.into(), person: "ada".into(), month: "2026-11".into(), usd: 15.0, reason: "the quarterly report".into() };
    l.apply(at(2), &ask("q1"));
    assert!(l.pending("ada", "2026-11").is_some());
    let stop = left(&l, 0.0, 20.0, "ada", "2026-11").unwrap_err();
    assert!(stop.pending && !stop.can_ask, "she could ask again while one waits");
    assert!(stop.to_string().contains("waiting for an admin"), "{stop}");
    l.apply(at(3), &Change::IncreaseDecided { id: "q1".into(), approved: true, usd: 10.0, by: "sam".into(), note: String::new() });
    assert_eq!(l.person_limit("ada", "2026-11"), Some(30.0), "the approved amount was not added");
    assert_eq!(l.person_limit("ada", "2026-12"), Some(20.0), "the increase outlived its month");
    assert_eq!(left(&l, 0.0, 20.0, "ada", "2026-11"), Ok(Some(10.0)));
    l.apply(at(4), &Change::IncreaseDecided { id: "q1".into(), approved: true, usd: 99.0, by: "sam".into(), note: String::new() });
    assert_eq!(l.person_limit("ada", "2026-11"), Some(30.0), "a request was decided twice");
    l.apply(at(5), &ask("q2"));
    l.apply(at(6), &Change::IncreaseDecided { id: "q2".into(), approved: false, usd: 0.0, by: "sam".into(), note: "not this month".into() });
    assert_eq!(l.person_limit("ada", "2026-11"), Some(30.0), "a declined request raised the limit");
    assert!(l.pending("ada", "2026-11").is_none());
    assert_eq!(l.requests["q2"].decided.as_ref().map(|d| (d.approved, d.note.as_str())), Some((false, "not this month")));
}

/// A role's rule limits everyone with it; a person's own rule wins; the
/// owner is never limited; a role without a rule adds nothing.
#[test]
fn model_access_follows_roles_and_people() {
    let mut a = Access::default();
    let member = vec!["member".to_string()];
    let admin = vec!["admin".to_string(), "member".to_string()];
    assert_eq!(a.allowed("ada", &member), None, "with no rule, every model");
    a.set(Some("member"), None, Some(vec!["a/cheap".into()]));
    a.set(Some("admin"), None, Some(vec!["a/costly".into()]));
    assert_eq!(a.allowed("ada", &member).unwrap().into_iter().collect::<Vec<_>>(), vec!["a/cheap"]);
    assert_eq!(a.allowed("sam", &admin).unwrap().len(), 2, "an admin has both roles' models");
    assert_eq!(a.allowed("dev", &["developer".to_string(), "member".to_string()]).unwrap().len(), 1, "a role with no rule opened every model");
    a.set(None, Some("ada"), Some(vec!["b/special".into()]));
    assert_eq!(a.allowed("ada", &member).unwrap().into_iter().collect::<Vec<_>>(), vec!["b/special"], "her own rule wins");
    assert_eq!(a.allowed("own", &["owner".to_string()]), None, "the owner was limited");
    a.set(None, Some("ada"), None);
    a.set(Some("member"), None, None);
    assert_eq!(a.allowed("ada", &member), None, "removing the rules opens every model again");
}

/// Enforced at selection: a model the person chose but may not use is
/// refused with the ones they may; with none chosen, the default, or the
/// first they may use when the default is not theirs.
#[test]
fn a_model_not_allowed_is_refused_with_those_that_are() {
    let allowed = Some(["a/cheap".to_string(), "b/mid".to_string()].into_iter().collect());
    let default = ("a".to_string(), "costly".to_string());
    let e = choose(&allowed, Some(("a".into(), "costly".into())), default.clone()).unwrap_err();
    assert_eq!(e, ModelNotAllowed { model: "a/costly".into(), allowed: vec!["a/cheap".into(), "b/mid".into()] });
    assert_eq!(e.to_string(), "You may not use a/costly. You can use: a/cheap, b/mid.");
    assert_eq!(choose(&allowed, Some(("b".into(), "mid".into())), default.clone()), Ok(("b".into(), "mid".into())));
    assert_eq!(choose(&allowed, None, default.clone()), Ok(("a".into(), "cheap".into())), "the default was used though not allowed");
    assert_eq!(choose(&None, None, default.clone()), Ok(default.clone()));
    assert!(choose(&Some(Default::default()), None, default).is_err(), "nothing allowed, yet a model was chosen");
}

/// A refused start reaches the app as 402 with the spend details, or 403
/// with the models allowed.
#[test]
fn a_refused_start_tells_the_app_what_to_offer() {
    let stop = SpendLimited { which: "person", month: "2026-11".into(), spent_usd: 20.0, limit_usd: 20.0, can_ask: true, pending: false };
    let r = routes::refused(&anyhow::Error::new(stop)).unwrap();
    assert_eq!(r.status(), axum::http::StatusCode::PAYMENT_REQUIRED);
    let m = ModelNotAllowed { model: "a/x".into(), allowed: vec!["a/y".into()] };
    assert_eq!(routes::refused(&anyhow::Error::new(m)).unwrap().status(), axum::http::StatusCode::FORBIDDEN);
    assert!(routes::refused(&anyhow::anyhow!("other")).is_none());
}

/// The events read back as written, and a request's reason and a decision's
/// note are content, kept out of the log's record.
#[test]
fn spend_events_round_trip_and_keep_words_as_content() {
    let ev = crate::events::Ev::Spend(Change::IncreaseAsked { id: "q".into(), person: "ada".into(), month: "2026-11".into(), usd: 5.0, reason: "a report".into() });
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!((v["kind"].as_str(), v["change"].as_str()), (Some("spend"), Some("increase_asked")));
    let (record, content) = crate::log::split(v.clone());
    assert!(record.get("reason").is_none(), "the reason stayed in the record");
    assert_eq!(content.unwrap().body["reason"], "a report");
    let mut without = v;
    without.as_object_mut().unwrap().remove("reason");
    let back: crate::events::Ev = serde_json::from_value(without).unwrap();
    assert!(matches!(back, crate::events::Ev::Spend(Change::IncreaseAsked { .. })), "an erased reason no longer reads");
    for c in
        [limit(Scope::Server, 1.0), limit(Scope::Person("ada".into()), 2.0), Change::ModelsAllowed { role: Some("member".into()), person: None, models: Some(vec!["a/m".into()]) }]
    {
        let v = serde_json::to_value(crate::events::Ev::Spend(c)).unwrap();
        let _: crate::events::Ev = serde_json::from_value(v).unwrap();
    }
}
