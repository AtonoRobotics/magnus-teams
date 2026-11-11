//! Administration's spec lines that an audit found open (spec rev 141,
//! Administration), each against a kernel booted on a fresh database (the
//! harness in `tests.rs`): sam owns the server, ana is an admin by the
//! config, lee by the directory, and mo is a member.

use super::guard::mark;
use super::tests::{server, Server};
use axum::Router;
use chrono::Utc;
use serde_json::json;

/// A session that has just confirmed its password.
pub(super) async fn stepped(s: &Server, id: &str) -> String {
    let t = s.token(id).await;
    mark(&t, Utc::now());
    t
}

/// People: every admin read of a person's data is logged and the person told.
/// The schedules list an admin sees to suspend one carries no words of the
/// person's: what a schedule does is read through the recorded data read.
#[tokio::test(flavor = "multi_thread")]
async fn an_admins_schedule_list_carries_none_of_the_persons_words() {
    let s = server(|_| Router::new()).await;
    let mo = s.token("mo").await;
    let (st, v) = s.post(&mo, "/v1/schedules", json!({ "text": "Chase the bank about the private loan", "daily_at": "09:00" })).await;
    assert!(st < 300, "{v}");
    let ana = stepped(&s, "ana").await;
    let (st, v) = s.get(&ana, "/v1/admin/people/mo/schedules").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["schedules"].as_array().map(Vec::len), Some(1), "{v}");
    assert!(!v.to_string().contains("private loan"), "an admin read a person's schedule with no record and no notice: {v}");
    s.done().await;
}

/// Overview: "anything that needs an admin: an agent stopped". One person's
/// switch, pulled with no step-up, is an alert naming who pulled it.
#[tokio::test(flavor = "multi_thread")]
async fn overview_shows_an_agent_stopped() {
    let s = server(|_| Router::new()).await;
    let ana = s.token("ana").await;
    let (st, v) = s.post(&ana, "/v1/admin/kill-switch/person/mo/press", json!({})).await;
    assert_eq!(st, 200, "{v}");
    let (st, o) = s.get(&ana, "/v1/admin/overview").await;
    assert_eq!(st, 200, "{o}");
    let alerts: Vec<String> = o["alerts"].as_array().into_iter().flatten().filter_map(|a| a["what"].as_str().map(String::from)).collect();
    assert!(alerts.iter().any(|a| a.starts_with("Ana stopped Mo's agents")), "a stopped person is not in Overview's alerts: {alerts:?}");
    assert_eq!(o["stops"]["on"].as_array().map(Vec::len), Some(1), "{o}");
    s.done().await;
}

/// "magnus kill on the server itself still stops everything when the app or
/// sign-in is broken": through the control socket alone, the server's switch
/// goes on, recorded as the server's operator with an audit entry, and the
/// banner names them; `--release` turns it off the same way.
#[tokio::test(flavor = "multi_thread")]
async fn magnus_kill_on_the_server_stops_everything() {
    let s = server(|_| Router::new()).await;
    let dir = s.k.cfg.appliance.state_dir.clone();
    let l = crate::control::bind(&dir).unwrap();
    tokio::spawn(crate::control::serve(s.k.clone(), l));
    let said = crate::kill_cli::kill(&dir, "root", false).await.unwrap();
    assert!(said.starts_with("The kill switch is on"), "{said}");
    let by = s.k.read(|st| (st.kill_switch, st.kill_switch_by.as_ref().map(|(a, _)| a.clone())));
    assert_eq!(by, (true, Some("server:root".to_string())), "the switch is not on, by the server's operator");
    let engaged = s.audited("kill_switch_engaged");
    assert_eq!((engaged.len(), engaged[0]["admin"].as_str(), engaged[0]["source"].as_str()), (1, Some("server:root"), Some("server")), "{engaged:?}");
    let mo = s.token("mo").await;
    let (_, v) = s.get(&mo, "/v1/stops").await;
    assert_eq!(v["stops"][0]["by_name"], "root on the server", "{v}");
    crate::kill_cli::kill(&dir, "root", true).await.unwrap();
    assert!(!s.k.read(|st| st.kill_switch), "magnus kill --release left the switch on");
    assert_eq!(s.audited("kill_switch_released").len(), 1);
    s.done().await;
}

/// Kill switch: "a banner shows who pulled it and when, to the admins and to
/// the people affected". An admin's banner names every switch that is on,
/// also one that stops someone else's agents; the person stopped sees theirs.
#[tokio::test(flavor = "multi_thread")]
async fn every_admin_sees_who_pulled_every_switch() {
    let s = server(|_| Router::new()).await;
    let sam = s.token("sam").await;
    assert_eq!(s.post(&sam, "/v1/admin/kill-switch/person/mo/press", json!({})).await.0, 200);
    for admin in ["ana", "lee"] {
        let t = s.token(admin).await;
        let (_, v) = s.get(&t, "/v1/stops").await;
        let stop = v["stops"].as_array().into_iter().flatten().find(|x| x["level"] == "person" && x["target"] == "mo").cloned();
        let stop = stop.unwrap_or_else(|| panic!("{admin}'s banner does not show mo's switch: {v}"));
        assert_eq!((stop["by_name"].as_str(), stop["mine"].as_bool(), stop["target_name"].as_str()), (Some("Sam"), Some(false), Some("Mo")), "{stop}");
        assert!(stop["at"].is_string(), "{stop}");
    }
    let mo = s.token("mo").await;
    let (_, v) = s.get(&mo, "/v1/stops").await;
    assert_eq!(v["stops"][0]["mine"], true, "{v}");
    s.done().await;
}
