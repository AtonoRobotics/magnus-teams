//! One rule for who reads a project and who posts in its conversations
//! (2026-11-23-one-rule-for-project-and-room-access): a project deleted
//! with its owner opens to no one; someone taken off a project can no
//! longer release, steer or share into its conversation; and a mission is
//! recorded only once every one of its lanes passes. sam owns the
//! workspace and mo is a member.

use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, LaneKind, TeamMissionEvent};
use crate::kernel::NewRun;
use crate::team::Change;
use axum::Router;
use serde_json::json;

async fn workspace(s: &Server) -> (String, String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    (ws, sam, s.token("mo").await)
}

/// A project of the caller's, with these people, and a conversation tied to it.
async fn project_room(s: &Server, ws: &str, owner: &str, members: &[&str]) -> (String, String) {
    let (st, v) = s.post(owner, &format!("/v1/workspaces/{ws}/projects"), json!({ "name": "Secret", "members": members })).await;
    assert_eq!(st, 200, "{v}");
    let project = v["id"].as_str().unwrap().to_string();
    let channel = s.post(owner, &format!("/v1/workspaces/{ws}/channels"), json!({ "name": "deal" })).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(owner, &format!("/v1/workspaces/{ws}/channels/{channel}/project"), json!({ "project": project })).await.0, 200);
    (project, channel)
}

/// Removing a person with their projects deleted untied each project's
/// channels, and a channel, a file or a task naming no project was open to
/// every member: the private project's history fell open to the workspace.
/// Now its channels go with it, and what else names it opens to no one,
/// the workspace's owner included.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_deleted_with_its_owner_opens_to_no_one() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo) = workspace(&s).await;
    let (project, channel) = project_room(&s, &ws, &mo, &[]).await;
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "the price is 4m" })).await.0, 200);
    let put = reqwest::Client::new().put(format!("{}/v1/workspaces/{ws}/files/terms.txt?project={project}", s.base)).bearer_auth(&mo).body("terms").send().await.unwrap();
    assert_eq!(put.status().as_u16(), 200);
    let (st, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/tasks"), json!({ "title": "close the deal", "project": project })).await;
    assert_eq!(st, 200, "{v}");
    let task = v["id"].as_str().unwrap().to_string();

    // What `DELETE /v1/admin/people/mo` with `projects: "delete"` writes.
    let removal = s.k.read(|st| {
        let held = crate::admin_app::remove::held(&st.team, "mo");
        crate::admin_app::remove::changes(&st.team, "mo", &held, None)
    });
    assert!(removal.iter().any(|c| matches!(c, Change::ProjectRemoved { .. })), "the removal deletes no project: {removal:?}");
    for c in removal {
        s.k.emit("person:ana", Ev::Team(c)).await.unwrap();
    }

    let (_, v) = s.get(&sam, &format!("/v1/workspaces/{ws}")).await;
    assert!(!v["channels"].as_array().unwrap().iter().any(|c| c["id"] == channel.as_str()), "the deleted project's channel is listed: {v}");
    assert_eq!(s.get(&sam, &format!("/v1/workspaces/{ws}/channels/{channel}/messages")).await.0, 404, "the deleted project's messages were read");
    let file = reqwest::Client::new().get(format!("{}/v1/workspaces/{ws}/files/terms.txt", s.base)).bearer_auth(&sam).send().await.unwrap();
    assert_eq!(file.status().as_u16(), 404, "the deleted project's file was read");
    let (st, v) = s.call(&sam, reqwest::Method::PUT, &format!("/v1/workspaces/{ws}/tasks/{task}"), Some(json!({ "status": "done" }))).await;
    assert_eq!(st, 404, "the deleted project's task was open: {v}");
    let (_, v) = s.get(&sam, &format!("/v1/workspaces/{ws}/search?q=price")).await;
    assert!(!v.to_string().contains("4m"), "search found the deleted project's words: {v}");
    let (_, v) = s.get(&sam, &format!("/v1/workspaces/{ws}/search?q=deal")).await;
    assert!(!v.to_string().contains("close the deal"), "search found the deleted project's task: {v}");
    s.done().await;
}

/// A lane of mo's in a project conversation, waiting for mo to release it.
async fn drafted_lane(s: &Server, ws: &str, room: &str) {
    let started = TeamMissionEvent::Started { workspace: ws.into(), room: room.into(), mission: "m1".into(), goal: "g".into(), requester: "sam".into(), leader_run: None };
    s.k.emit("person:sam", Ev::TeamMission(started)).await.unwrap();
    let dispatched = TeamMissionEvent::LaneDispatched {
        workspace: ws.into(),
        mission: "m1".into(),
        lane: "l1".into(),
        kind: LaneKind::MemberAgent,
        target_member: Some("mo".into()),
        objective: "the latest numbers".into(),
        scope: String::new(),
        auto_release: false,
        child_task: None,
        child_run: None,
        thread: None,
        routine: None,
    };
    s.k.emit("person:sam", Ev::TeamMission(dispatched)).await.unwrap();
    let draft = TeamMissionEvent::LaneDraftReady {
        workspace: ws.into(),
        mission: "m1".into(),
        lane: "l1".into(),
        owner: "mo".into(),
        draft: "Revenue is up.".into(),
        scrub_flagged: false,
        held_because: None,
    };
    s.k.emit("kernel", Ev::TeamMission(draft)).await.unwrap();
}

/// Releasing a lane, steering it and sharing a whisper checked only that
/// the room was there, not that its owner could still see it: someone taken
/// off a project still posted into its conversation. Each now asks the one
/// room-access check.
#[tokio::test(flavor = "multi_thread")]
async fn someone_taken_off_a_project_no_longer_posts_in_its_conversation() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo) = workspace(&s).await;
    let (project, channel) = project_room(&s, &ws, &sam, &["mo"]).await;
    drafted_lane(&s, &ws, &channel).await;
    let (task, run) = s.k.create_run("person:mo", "mo", "what does it say".into(), vec![], NewRun::default()).await.unwrap();
    s.k.emit("kernel", Ev::Completed { run, summary: "It says the price is 4m.".into(), view: None, spoken: None }).await.unwrap();
    let whisper = Change::Whisper { workspace: ws.clone(), channel: channel.clone(), thread: None, task: task.clone(), owner: "mo".into(), history: String::new() };
    s.k.emit("person:mo", Ev::Team(whisper)).await.unwrap();

    assert_eq!(s.call(&sam, reqwest::Method::PUT, &format!("/v1/workspaces/{ws}/projects/{project}"), Some(json!({ "members": [] }))).await.0, 200);
    let runs = s.k.read(|st| st.runs.len());
    let lane = format!("/v1/workspaces/{ws}/missions/m1/lanes/l1");
    let (st, v) = s.post(&mo, &format!("{lane}/steer"), json!({ "instruction": "shorter" })).await;
    assert_eq!(st, 403, "a steer went ahead for someone off the project: {v}");
    assert_eq!(s.k.read(|st| st.runs.len()), runs, "a refused steer started a run");
    let (st, v) = s.post(&mo, &format!("{lane}/release"), json!({})).await;
    assert_eq!(st, 403, "a release went ahead for someone off the project: {v}");
    let (st, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/whispers/{task}/share"), json!({})).await;
    assert_eq!(st, 403, "a whisper was shared by someone off the project: {v}");
    assert!(s.k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.is_empty()), "someone off the project posted in its conversation");
    s.done().await;
}

/// A mission's `Started` was written before its lanes were checked: lanes
/// [valid, bad] answered 400, but the valid lane was already running and
/// spending, and the mission stayed for ever. Now nothing is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_mission_with_one_bad_lane_records_nothing() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, _) = workspace(&s).await;
    let room = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let runs = s.k.read(|st| st.runs.len());
    let lanes = [
        json!([{ "kind": "member_agent", "objective": "the latest numbers" }, { "kind": "member_agent", "objective": "" }]),
        json!([{ "kind": "member_agent", "objective": "the latest numbers" }, { "kind": "member_agent", "target_member": "nobody", "objective": "theirs" }]),
    ];
    for lanes in lanes {
        let (st, v) = s.post(&sam, &format!("/v1/workspaces/{ws}/rooms/{room}/missions"), json!({ "goal": "a report", "lanes": lanes })).await;
        assert_eq!(st, 400, "a mission with a bad lane was taken: {v}");
    }
    assert!(s.k.read(|st| st.missions.view_for_room(&ws, &room, "sam")).is_empty(), "a refused mission was recorded");
    assert_eq!(s.k.read(|st| st.runs.len()), runs, "a lane of a refused mission ran");
    s.done().await;
}
