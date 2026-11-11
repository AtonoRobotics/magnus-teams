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

/// "Each admin change asks for the admin's password and code again": the
/// app's older owner routes outside the Admin section (Settings' roles,
/// models, connectors, updates and kill switch) change nothing for an admin
/// without a step-up, and each change is in the admin audit. Pressing the
/// kill switch is the one change that needs none.
#[tokio::test(flavor = "multi_thread")]
async fn settings_routes_take_the_admin_step_up_and_audit() {
    let s = server(|_| Router::new()).await;
    let sam = s.token("sam").await;
    let roles = json!({ "roles": ["developer"] });
    let (st, v) = s.call(&sam, reqwest::Method::PUT, "/v1/people/mo/roles", Some(roles.clone())).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "a role changed with no step-up: {v}");
    let (st, v) = s.call(&sam, reqwest::Method::PUT, "/v1/model", Some(json!({ "adapter": "a", "model": "m" }))).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "the default model changed with no step-up: {v}");
    let (st, v) = s.post(&sam, "/v1/updates/rollback", json!({})).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "a rollback started with no step-up: {v}");
    // Pressing stops at once; releasing asks.
    let (st, v) = s.post(&sam, "/v1/kill-switch", json!({ "engaged": true })).await;
    assert_eq!(st, 200, "{v}");
    assert!(s.k.read(|x| x.kill_switch));
    assert_eq!(s.audited("kill_switch_engaged").len(), 1, "pressing is not in the admin audit");
    let (st, v) = s.post(&sam, "/v1/kill-switch", json!({ "engaged": false })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "released with no step-up: {v}");
    assert!(s.k.read(|x| x.kill_switch), "the switch was released with no step-up");
    mark(&sam, Utc::now());
    assert_eq!(s.post(&sam, "/v1/kill-switch", json!({ "engaged": false })).await.0, 200);
    assert_eq!(s.audited("kill_switch_released").len(), 1);
    let (st, v) = s.call(&sam, reqwest::Method::PUT, "/v1/people/mo/roles", Some(roles)).await;
    assert_eq!(st, 204, "{v}");
    let changed = s.audited("role_changed");
    assert_eq!(changed.len(), 1, "a role change from Settings is not in the admin audit");
    assert_eq!(changed[0]["target"], "/v1/people/mo/roles");
    // Someone who is no admin is refused by the route, as before.
    let mo = s.token("mo").await;
    let (st, v) = s.call(&mo, reqwest::Method::PUT, "/v1/data-access", Some(json!({ "private": true }))).await;
    assert!(st == 403 && v["error"] != "step_up", "{st} {v}");
    s.done().await;
}

/// Operations › Updates sets the server's own channel, beside the desktop
/// apps': the server then takes its releases from that channel, or from
/// production while the channel has none. Setting it takes the step-up and
/// is in the admin audit.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_sets_the_servers_own_update_channel() {
    let s = server(|_| Router::new()).await;
    let appliance = s.k.cfg.updates_dir().join("appliance");
    for (dir, version) in [(appliance.clone(), "9.0.0"), (appliance.join("test"), "9.1.0-test.1")] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("latest.json"), json!({ "version": version, "file": "x.tar.gz", "signature": "" }).to_string()).unwrap();
    }
    let version = || s.k.latest_release().map(|m| m["version"].clone());
    assert_eq!(version(), Some(json!("9.0.0")));
    let ana = s.token("ana").await;
    let (st, v) = s.get(&ana, "/v1/admin/updates").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["server"]["channel"], "production", "{v}");
    assert_eq!(v["server"]["channels"].as_array().map(|c| c.len()), Some(3), "{v}");
    async fn put(s: &Server, t: &str, c: &str) -> (u16, serde_json::Value) {
        s.call(t, reqwest::Method::PUT, "/v1/admin/updates/server-channel", Some(json!({ "channel": c }))).await
    }
    let put = |t, c| put(&s, t, c);
    let (st, v) = put(&ana, "test").await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "the channel changed with no step-up: {v}");
    mark(&ana, Utc::now());
    assert_eq!(put(&ana, "nightly").await.0, 400, "a channel that does not exist was taken");
    assert_eq!(put(&ana, "test").await.0, 200);
    assert_eq!(version(), Some(json!("9.1.0-test.1")), "the server does not follow its channel");
    let set = s.audited("server_channel_set");
    assert_eq!(set.len(), 1, "the channel change is not in the admin audit");
    assert_eq!((set[0]["before"]["channel"].as_str(), set[0]["after"]["channel"].as_str()), (Some("production"), Some("test")));
    // A channel with nothing published follows production.
    assert_eq!(put(&ana, "development").await.0, 200);
    assert_eq!(version(), Some(json!("9.0.0")));
    assert_eq!(s.get(&ana, "/v1/admin/updates").await.1["server"]["channel"], "development");
    let mo = s.token("mo").await;
    assert_eq!(put(&mo, "test").await.0, 403, "a member set the server's channel");
    s.done().await;
}

/// Security sets how long the trash and the backups keep things, within the
/// purge window; a shorter window is swept at once, and the change is in the
/// admin audit.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_sets_how_long_things_are_kept_within_the_purge_window() {
    let s = server(|_| Router::new()).await;
    let dir = s.k.cfg.backup_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let backup = |days_ago: i64| dir.join(format!("magnus-{}.backup", (Utc::now() - chrono::Duration::days(days_ago)).format("%Y%m%dT%H%M%S%.3fZ")));
    let (old, recent) = (backup(3), backup(1));
    for f in [&old, &recent] {
        std::fs::write(f, "a backup").unwrap();
    }
    let sam = s.token("sam").await;
    async fn put(s: &Server, t: &str, b: serde_json::Value) -> (u16, serde_json::Value) {
        s.call(t, reqwest::Method::PUT, "/v1/admin/retention", Some(b)).await
    }
    let (st, v) = put(&s, &sam, json!({ "trash_days": 7, "backup_days": 2 })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("step_up")), "retention changed with no step-up: {v}");
    mark(&sam, Utc::now());
    for bad in
        [json!({ "trash_days": 31, "backup_days": 2 }), json!({ "trash_days": 7, "backup_days": 31 }), json!({ "trash_days": 0, "backup_days": 2 }), json!({ "trash_days": 7 })]
    {
        assert_eq!(put(&s, &sam, bad.clone()).await.0, 400, "taken: {bad}");
    }
    assert!(old.exists(), "a refused change swept");
    let (st, v) = put(&s, &sam, json!({ "trash_days": 7, "backup_days": 2 })).await;
    assert_eq!(st, 200, "{v}");
    assert!(!old.exists(), "a backup past the new window was kept");
    assert!(recent.exists(), "a backup inside the window was removed");
    let set = s.audited("retention_set");
    assert_eq!(set.len(), 1, "the change is not in the admin audit");
    assert_eq!(set[0]["before"], json!({ "trash_days": 30, "backup_days": 30 }));
    assert_eq!(set[0]["after"], json!({ "trash_days": 7, "backup_days": 2 }));
    let (_, r) = s.get(&sam, "/v1/admin/retention").await;
    assert_eq!((r["trash_days"].as_u64(), r["backup_days"].as_u64()), (Some(7), Some(2)), "{r}");
    // The trash sweep and what a person is told go by it.
    assert_eq!(s.k.trash_keep_days(), 7);
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/trash").await.1["keep_days"], 7);
    assert_eq!(put(&s, &mo, json!({ "trash_days": 7, "backup_days": 2 })).await.0, 403, "a member set retention");
    s.done().await;
}
