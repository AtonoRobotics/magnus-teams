//! A lane's steps go through the ledger's one table (`LaneStatus::next`):
//! the owner cannot release a lane its worker still holds, a restart
//! follows again the lanes it left working, and an open-web lane's release
//! is its requester's, so erasing them reaches it.

use super::missions::{Lane, LaneStatus};
use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, LaneKind, TeamMissionEvent};
use crate::kernel::{Kernel, NewRun};
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;

const WAIT_TICK: std::time::Duration = std::time::Duration::from_millis(50);
const WAIT_TICKS: u32 = 200;

fn pg() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

/// A workspace sam owns, and its first channel.
async fn workspace(s: &Server) -> (String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    (ws, channel)
}

async fn started(k: &Kernel, ws: &str, room: &str, mission: &str) {
    let ev = TeamMissionEvent::Started { workspace: ws.into(), room: room.into(), mission: mission.into(), goal: "g".into(), requester: "sam".into(), leader_run: None };
    k.emit("person:sam", Ev::TeamMission(ev)).await.unwrap();
}

/// A lane dispatched without a watcher, as a restart leaves every lane.
#[allow(clippy::too_many_arguments)]
async fn dispatched(k: &Kernel, ws: &str, mission: &str, lane: &str, kind: LaneKind, owner: Option<&str>, run: Option<&str>, auto_release: bool, thread: Option<&str>) {
    let ev = TeamMissionEvent::LaneDispatched {
        workspace: ws.into(),
        mission: mission.into(),
        lane: lane.into(),
        kind,
        target_member: owner.map(String::from),
        objective: "look it up".into(),
        scope: String::new(),
        auto_release,
        child_task: None,
        child_run: run.map(String::from),
        thread: thread.map(String::from),
    };
    k.emit("person:sam", Ev::TeamMission(ev)).await.unwrap();
}

/// A run of sam's that nothing drives.
async fn run(k: &Arc<Kernel>) -> String {
    k.create_run("person:sam", "sam", "look it up".into(), vec![], NewRun::default()).await.unwrap().1
}

fn lane(k: &Kernel, mission: &str, lane: &str) -> Lane {
    k.read(|s| s.missions.mission(mission).map(|m| m.lanes[lane].clone())).unwrap()
}

/// Waits until the lane is no longer working.
async fn ended(k: &Kernel, mission: &str, id: &str) -> Lane {
    for _ in 0..WAIT_TICKS {
        let l = lane(k, mission, id);
        if l.status != LaneStatus::Working {
            return l;
        }
        tokio::time::sleep(WAIT_TICK).await;
    }
    panic!("lane {id} is still working");
}

/// The owner released a lane still working: it raced the watcher's own
/// release, and the room was posted to and the deliverable redone though
/// the ledger dropped the second release.
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_cannot_release_a_lane_still_working() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, channel) = workspace(&s).await;
    started(&s.k, &ws, &channel, "m1").await;
    dispatched(&s.k, &ws, "m1", "l1", LaneKind::MemberAgent, Some("sam"), None, false, None).await;
    let sam = s.token("sam").await;
    let path = format!("/v1/workspaces/{ws}/missions/m1/lanes/l1/release");
    let (st, v) = s.post(&sam, &path, json!({ "contribution": "my own words" })).await;
    assert_eq!(st, 404, "a lane still working was released: {v}");
    assert_eq!(lane(&s.k, "m1", "l1").status, LaneStatus::Working);
    assert!(s.k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.is_empty()), "the refused release was posted");

    let draft =
        TeamMissionEvent::LaneDraftReady { workspace: ws.clone(), mission: "m1".into(), lane: "l1".into(), owner: "sam".into(), draft: "the draft".into(), scrub_flagged: false };
    s.k.emit("kernel", Ev::TeamMission(draft)).await.unwrap();
    let (st, v) = s.post(&sam, &path, json!({})).await;
    assert_eq!(st, 200, "a draft could not be released: {v}");
    assert_eq!(lane(&s.k, "m1", "l1").contribution.as_deref(), Some("the draft"));
    assert_eq!(s.post(&sam, &path, json!({})).await.0, 404, "a released lane was released again");
    s.done().await;
}

/// An open-web lane's release was credited to `open_web`, no one, so the
/// room post and the release were left behind when its requester was
/// erased. It is the requester's now, and old rows are given to them.
#[tokio::test(flavor = "multi_thread")]
async fn an_open_web_release_is_its_requesters() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, channel) = workspace(&s).await;
    started(&s.k, &ws, &channel, "m1").await;
    let web = run(&s.k).await;
    dispatched(&s.k, &ws, "m1", "web", LaneKind::OpenWeb, None, Some(&web), true, None).await;
    s.k.spawn_lane_watcher(ws.clone(), "m1".into(), "web".into(), None, web.clone(), true, None);
    s.k.emit("kernel", Ev::Completed { run: web, summary: "The launch is on the 3rd.".into(), view: None, spoken: None }).await.unwrap();
    let l = ended(&s.k, "m1", "web").await;
    assert_eq!((l.status, l.released_by.as_deref()), (LaneStatus::Released, Some("sam")), "the open-web release was not its requester's");
    // The release is written before its room post: wait for the post.
    let messages = || s.k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.clone());
    for _ in 0..WAIT_TICKS {
        if !messages().is_empty() {
            break;
        }
        tokio::time::sleep(WAIT_TICK).await;
    }
    let posted = messages();
    assert_eq!(posted.iter().map(|m| m.sender.as_str()).collect::<Vec<_>>(), vec!["agent:sam"]);
    let erased = s.k.log.person_content_seqs("sam").await.unwrap();
    let released = s.k.log.of_kinds(&["team_mission"], 50).await.unwrap().into_iter().find(|(_, _, _, b)| b["change"] == "lane_released").unwrap().0;
    assert!(erased.contains(&released), "erasing the requester misses the release");

    // An open-web lane whose result cannot be shared has no Airlock to wait in: it fails.
    let held = run(&s.k).await;
    dispatched(&s.k, &ws, "m1", "held", LaneKind::OpenWeb, None, Some(&held), true, None).await;
    s.k.spawn_lane_watcher(ws.clone(), "m1".into(), "held".into(), None, held.clone(), true, None);
    s.k.emit("kernel", Ev::Completed { run: held, summary: "Their SSN is 123-45-6789.".into(), view: None, spoken: None }).await.unwrap();
    assert_eq!(ended(&s.k, "m1", "held").await.status, LaneStatus::Failed, "an unshareable open-web result left its lane working");

    // Rows an older release wrote for `open_web`.
    let old =
        TeamMissionEvent::LaneReleased { workspace: ws.clone(), mission: "m1".into(), lane: "old".into(), contribution: "Old findings.".into(), released_by: "open_web".into() };
    let old = s.k.emit("kernel", Ev::TeamMission(old)).await.unwrap().seq;
    let message = super::Message { id: "msg_old".into(), sender: "agent:open_web".into(), text: "Old findings.".into(), thread: None, at: chrono::Utc::now() };
    let posted = s.k.emit("person:open_web", Ev::Team(super::Change::Message { workspace: ws.clone(), channel: channel.clone(), message })).await.unwrap().seq;
    let c = s.store().await;
    c.execute("UPDATE public.content SET person = 'open_web' WHERE seq = ANY($1)", &[&vec![old, posted]]).await.unwrap();
    c.batch_execute(crate::team::lanes::OPEN_WEB_OWNERS).await.unwrap();
    let erased = s.k.log.person_content_seqs("sam").await.unwrap();
    assert!(erased.contains(&old) && erased.contains(&posted), "an older open-web release is still no one's");
    s.done().await;
}

/// A second server booted on a copy of `s`'s log: `s` restarted.
async fn restarted(s: &Server) -> (Arc<Kernel>, String, tempfile::TempDir) {
    let name = format!("lanes_{}", uuid::Uuid::new_v4().simple());
    let admin = crate::log::connect(&pg()).await.unwrap();
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
    let mut u = url::Url::parse(&pg()).unwrap();
    u.set_path(&format!("/{name}"));
    let c = crate::log::connect(u.as_str()).await.unwrap();
    c.batch_execute(
        "CREATE TABLE public.events (seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, at timestamptz NOT NULL DEFAULT now(), kind text NOT NULL, actor text NOT NULL, run_id text, body jsonb NOT NULL);
         CREATE TABLE public.content (seq bigint PRIMARY KEY REFERENCES public.events (seq), salt bytea NOT NULL, body jsonb NOT NULL, person text);",
    )
    .await
    .unwrap();
    c.batch_execute(&crate::accounts::migrate_sql("postgres")).await.unwrap();
    let from = s.store().await;
    for r in from.query("SELECT seq, at, kind, actor, run_id, body FROM public.events ORDER BY seq", &[]).await.unwrap() {
        let (seq, at, kind, actor, run, body): (i64, chrono::DateTime<chrono::Utc>, String, String, Option<String>, Value) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        let sql = "INSERT INTO public.events (seq, at, kind, actor, run_id, body) OVERRIDING SYSTEM VALUE VALUES ($1, $2, $3, $4, $5, $6)";
        c.execute(sql, &[&seq, &at, &kind, &actor, &run, &body]).await.unwrap();
    }
    for r in from.query("SELECT seq, salt, body, person FROM public.content ORDER BY seq", &[]).await.unwrap() {
        let (seq, salt, body, person): (i64, Vec<u8>, Value, Option<String>) = (r.get(0), r.get(1), r.get(2), r.get(3));
        c.execute("INSERT INTO public.content (seq, salt, body, person) VALUES ($1, $2, $3, $4)", &[&seq, &salt, &body, &person]).await.unwrap();
    }
    c.batch_execute("SELECT setval(pg_get_serial_sequence('public.events', 'seq'), (SELECT max(seq) FROM public.events))").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = s.k.cfg.clone();
    cfg.database.url = u.to_string();
    cfg.appliance.state_dir = dir.path().join("state");
    (Kernel::boot(cfg).await.unwrap(), name, dir)
}

/// Watchers started only at dispatch, so a restart left every working lane
/// working for ever (and a dispatched lane was not even read back: its
/// lane kind took the event's `kind`). Now the boot follows each again: a
/// worker still running to its end, posted in the thread it was asked in;
/// a lane whose worker ended meanwhile, is gone or never was, fails.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_follows_the_lanes_it_left_working() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, channel) = workspace(&s).await;
    started(&s.k, &ws, &channel, "m1").await;
    let live = run(&s.k).await;
    dispatched(&s.k, &ws, "m1", "live", LaneKind::MemberAgent, Some("sam"), Some(&live), true, Some("msg_root")).await;
    let stopped = run(&s.k).await;
    dispatched(&s.k, &ws, "m1", "stopped", LaneKind::MemberAgent, Some("sam"), Some(&stopped), true, None).await;
    s.k.emit("person:sam", Ev::Cancelled { run: stopped, source: Some("button".into()), turn: None }).await.unwrap();
    let finished = run(&s.k).await;
    dispatched(&s.k, &ws, "m1", "finished", LaneKind::OpenWeb, None, Some(&finished), true, None).await;
    s.k.emit("kernel", Ev::Completed { run: finished, summary: "Old news.".into(), view: None, spoken: None }).await.unwrap();
    dispatched(&s.k, &ws, "m1", "gone", LaneKind::MemberAgent, Some("sam"), Some("run_gone"), true, None).await;
    dispatched(&s.k, &ws, "m1", "none", LaneKind::MemberAgent, Some("sam"), None, false, None).await;

    let (k, db, _dir) = restarted(&s).await;
    for gone in ["stopped", "finished"] {
        assert_eq!(ended(&k, "m1", gone).await.failure_reason.as_deref(), Some("its worker ended while the server was down"), "{gone}");
    }
    assert_eq!(ended(&k, "m1", "gone").await.failure_reason.as_deref(), Some("worker run disappeared"));
    assert_eq!(ended(&k, "m1", "none").await.status, LaneStatus::Failed, "a lane with no worker stayed working");
    assert_eq!(lane(&k, "m1", "live").status, LaneStatus::Working);
    k.emit("kernel", Ev::Completed { run: live, summary: "Done after the restart.".into(), view: None, spoken: None }).await.unwrap();
    assert_eq!(ended(&k, "m1", "live").await.status, LaneStatus::Released, "a lane still running at the restart was not followed");
    // The release is written before its room post: wait for the post.
    let messages = || k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.clone());
    let in_thread = |m: &super::Message| m.text == "Done after the restart." && m.thread.as_deref() == Some("msg_root");
    for _ in 0..WAIT_TICKS {
        if messages().iter().any(in_thread) {
            break;
        }
        tokio::time::sleep(WAIT_TICK).await;
    }
    let posted = messages();
    assert!(posted.iter().any(in_thread), "the result was not posted in its thread: {posted:?}");
    assert!(!posted.iter().any(|m| m.text == "Old news."), "a result from before the restart was posted late");

    crate::log::connect(&pg()).await.unwrap().batch_execute(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).await.unwrap();
    s.done().await;
}
