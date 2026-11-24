//! Every copy of a person's words goes with them: a whisper's thread, a
//! mission's goal, objectives and deliverable, live and after a replay.
//! sam owns the workspace; mo, a member from the config, is erased.

use super::Change;
use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, LaneKind, TeamMissionEvent};
use crate::state::State;
use axum::Router;
use reqwest::Method;
use serde_json::{json, Value};

/// A workspace sam owns with mo in it, and its general channel.
async fn workspace(s: &Server) -> (String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    let c = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.iter().find(|(_, c)| c.name == "general").map(|(id, _)| id.clone()).unwrap());
    (ws, c)
}

async fn post(s: &Server, who: &str, ws: &str, channel: &str, body: Value) -> Value {
    let t = s.token(who).await;
    let (st, v) = s.post(&t, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), body).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// The state as a restart reads it from the log.
async fn replayed(s: &Server) -> State {
    let mut st = State::default();
    for e in s.k.log.load().await.unwrap() {
        st.apply(&e);
    }
    st
}

/// Whether any row of these kinds still holds `text`, in its record or its content.
async fn logged(s: &Server, kinds: &[&str], text: &str) -> bool {
    let seqs: Vec<i64> = s.k.log.of_kinds(kinds, 500).await.unwrap().into_iter().map(|(seq, ..)| seq).collect();
    s.k.log.contents(&seqs).await.unwrap().values().any(|b| b.to_string().contains(text))
}

async fn mission(s: &Server, ev: TeamMissionEvent) -> i64 {
    s.k.emit("kernel", Ev::TeamMission(ev)).await.unwrap().seq
}

fn started(ws: &str, room: &str, requester: &str, goal: &str) -> TeamMissionEvent {
    TeamMissionEvent::Started { workspace: ws.into(), room: room.into(), mission: "m1".into(), goal: goal.into(), requester: requester.into(), leader_run: None }
}

fn lane(ws: &str, lane: &str, owner: &str, objective: &str) -> TeamMissionEvent {
    TeamMissionEvent::LaneDispatched {
        workspace: ws.into(),
        mission: "m1".into(),
        lane: lane.into(),
        kind: LaneKind::MemberAgent,
        target_member: Some(owner.into()),
        objective: objective.into(),
        scope: String::new(),
        auto_release: true,
        child_task: None,
        child_run: None,
        thread: None,
    }
}

/// The row as a release before 2026-11-22 wrote it: under its lane's kind,
/// its objective kept in the record.
async fn as_old_lane_row(s: &Server, seq: i64) {
    let c = s.store().await;
    let sql = "UPDATE public.events e SET kind = 'member_agent', body = (e.body - 'content' - 'lane_kind') || jsonb_build_object('kind', 'member_agent') || c.body
               FROM public.content c WHERE c.seq = e.seq AND e.seq = $1";
    assert_eq!(c.execute(sql, &[&seq]).await.unwrap(), 1);
    c.execute("DELETE FROM public.content WHERE seq = $1", &[&seq]).await.unwrap();
}

/// A whisper copied the last 40 messages of its thread, everyone's, into
/// its own event, which is its owner's: erasing mo left mo's words there,
/// and fed them to sam's agent. Now a whisper reads its thread as it
/// stands, and a copy an older release wrote goes with the words it quoted.
#[tokio::test(flavor = "multi_thread")]
async fn an_erased_members_words_leave_others_whispers() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    post(&s, "mo", &ws, &c, json!({ "text": "the door code is 4471" })).await;
    post(&s, "sam", &ws, &c, json!({ "text": "sam's own plan" })).await;
    let v = post(&s, "sam", &ws, &c, json!({ "text": "what did mo say?", "whisper": true })).await;
    let task = v["task_id"].as_str().unwrap().to_string();
    let run = s.k.read(|st| st.runs.get(v["run_id"].as_str().unwrap()).cloned()).unwrap();
    assert!(s.k.whisper_context(&run).unwrap().contains("mo: the door code is 4471"), "the whisper does not read its thread");
    let old = Change::Whisper {
        workspace: ws.clone(),
        channel: c.clone(),
        thread: None,
        task: "task_old".into(),
        owner: "sam".into(),
        history: "mo: the door code is 4471\nsam: sam's own plan".into(),
    };
    s.k.emit("person:sam", Ev::Team(old)).await.unwrap();

    s.k.erase_person("person:sam", "mo").await.unwrap();

    let context = s.k.whisper_context(&run).unwrap();
    assert!(!context.contains("4471"), "sam's whisper still holds mo's words: {context}");
    assert!(context.contains("sam: sam's own plan"), "sam's whisper lost sam's own words: {context}");
    assert!(!logged(&s, &["team"], "4471").await, "a whisper's copy of mo's words is still in the log");
    let (_, thread) = replayed(&s).await.team.whisper_thread(&task).unwrap();
    assert!(!thread.contains("4471") && thread.contains("sam's own plan"), "after a restart: {thread}");
    s.done().await;
}

/// A released lane's contribution was copied into the deliverable, whose
/// row is the requester's: erasing the contributor left it there.
#[tokio::test(flavor = "multi_thread")]
async fn an_erased_contributors_words_leave_the_deliverable() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    mission(&s, started(&ws, &c, "sam", "Board numbers")).await;
    mission(&s, lane(&ws, "l_mo", "mo", "your quarter figures")).await;
    let released =
        TeamMissionEvent::LaneReleased { workspace: ws.clone(), mission: "m1".into(), lane: "l_mo".into(), contribution: "mo's figures: 9931".into(), released_by: "mo".into() };
    mission(&s, released).await;
    s.k.refresh_mission_deliverable(&ws, "m1").await;
    // A deliverable as an older release wrote it, the contribution copied in.
    let old =
        TeamMissionEvent::DeliverableUpdated { workspace: ws.clone(), mission: "m1".into(), markdown: "### your quarter figures (mo)\nmo's figures: 9931".into(), complete: true };
    mission(&s, old).await;
    let view = |st: &State| st.missions.view_for_room(&ws, &c, "sam")[0].clone();
    assert!(s.k.read(view)["deliverable"].as_str().unwrap().contains("9931"), "the deliverable does not show the release");

    s.k.erase_person("person:sam", "mo").await.unwrap();

    let live = s.k.read(view);
    assert!(!live.to_string().contains("9931"), "the deliverable still shows mo's words: {live}");
    assert!(live["deliverable"].is_string() && live["goal"] == "Board numbers", "sam's mission lost its own words: {live}");
    assert!(!logged(&s, &["team_mission"], "9931").await, "a deliverable's copy of mo's words is still in the log");
    let after = view(&replayed(&s).await);
    assert!(!after.to_string().contains("9931"), "after a restart: {after}");
    s.done().await;
}

/// Deleting an `@Magnus` message for everyone took its text from the
/// channel but left it as the mission's goal and its lane's objective.
#[tokio::test(flavor = "multi_thread")]
async fn an_unsent_ask_leaves_its_mission() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    let v = post(&s, "sam", &ws, &c, json!({ "text": "@Magnus summarise the plum orchard plan", "agent": true })).await;
    assert!(v["mission_id"].is_string(), "the ask started no mission: {v}");
    let view = |st: &State| Value::from(st.missions.view_for_room(&ws, &c, "sam"));
    assert!(s.k.read(view).to_string().contains("plum orchard"), "the mission does not hold the ask");

    let sam = s.token("sam").await;
    let (st, r) = s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}/channels/{c}/messages/{}", v["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200, "{r}");

    let live = s.k.read(view);
    assert!(!live.to_string().contains("plum orchard"), "the deleted ask is still the mission's goal or objective: {live}");
    assert!(!logged(&s, &["team_mission"], "plum orchard").await, "the deleted ask is still in the mission's events");
    let after = view(&replayed(&s).await);
    assert!(!after.to_string().contains("plum orchard"), "after a restart: {after}");
    s.done().await;
}

/// Erasing the requester left their goal and objectives in the mission;
/// and a lane stored whole by an older release read back with its
/// objective after a restart, as no content row of theirs held it.
#[tokio::test(flavor = "multi_thread")]
async fn an_erased_requesters_goal_and_objectives_go() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    mission(&s, started(&ws, &c, "mo", "mo's secret launch date")).await;
    let old = mission(&s, lane(&ws, "l_old", "mo", "find mo's secret launch date")).await;
    as_old_lane_row(&s, old).await;
    mission(&s, lane(&ws, "l_new", "mo", "check mo's secret launch date")).await;
    let view = |st: &State| Value::from(st.missions.view_for_room(&ws, &c, "sam"));
    assert!(replayed(&s).await.missions.mission("m1").unwrap().lanes["l_old"].objective.contains("secret"), "the old lane did not read back");

    s.k.erase_person("person:sam", "mo").await.unwrap();

    let live = s.k.read(view);
    assert!(!live.to_string().contains("secret launch"), "mo's goal or objectives are still live: {live}");
    let after = replayed(&s).await;
    assert!(after.missions.mission("m1").unwrap().lanes.contains_key("l_old"), "the old lane was not read back");
    assert!(!view(&after).to_string().contains("secret launch"), "after a restart: {}", view(&after));
    s.done().await;
}

/// A lane stored whole by an older release kept its objective in the
/// record, which nothing can delete: `migrate` moves it to the content
/// store, as the requester's, where their erasure reaches it.
#[tokio::test(flavor = "multi_thread")]
async fn an_old_lanes_objective_moves_to_the_content_store() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    mission(&s, started(&ws, &c, "mo", "the goal")).await;
    let old = mission(&s, lane(&ws, "l_old", "mo", "find the launch date")).await;
    as_old_lane_row(&s, old).await;

    let store = s.store().await;
    crate::log::move_inline_content(&store).await.unwrap();
    store.batch_execute(crate::log::CONTENT_OWNERS).await.unwrap();

    let record: Value = store.query_one("SELECT body FROM public.events WHERE seq = $1", &[&old]).await.unwrap().get(0);
    assert!(record.get("objective").is_none(), "the objective is still in the record: {record}");
    assert!(s.k.log.person_content_seqs("mo").await.unwrap().contains(&old), "the moved objective is not the requester's");
    let after = replayed(&s).await;
    assert_eq!(after.missions.mission("m1").unwrap().lanes["l_old"].objective, "find the launch date", "the moved lane no longer reads back");
    s.done().await;
}

/// The `task_created` rows that hold `text`.
async fn tasks_holding(s: &Server, text: &str) -> usize {
    let seqs: Vec<i64> = s.k.log.of_kinds(&["task_created"], 500).await.unwrap().into_iter().map(|(seq, ..)| seq).collect();
    s.k.log.contents(&seqs).await.unwrap().values().filter(|b| b.to_string().contains(text)).count()
}

/// `who` steers the one lane of `mission`, which is theirs.
async fn steer(s: &Server, who: &str, ws: &str, mission: &str) {
    let lane = s.k.read(|st| st.missions.mission(mission).unwrap().lanes.keys().next().unwrap().clone());
    let t = s.token(who).await;
    let (st, v) = s.post(&t, &format!("/v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/steer"), json!({ "instruction": "shorter" })).await;
    assert_eq!(st, 200, "{v}");
}

/// A lane mo asked of sam's agent is a task of sam's whose text quotes mo's
/// request, and so is sam's steer of it: erasing mo left both there.
#[tokio::test(flavor = "multi_thread")]
async fn an_erased_requesters_words_leave_the_lane_tasks_of_others() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    let mo = s.token("mo").await;
    let lanes = json!([{ "kind": "member_agent", "target_member": "sam", "objective": "list the plum orchard invoices" }]);
    let (st, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/rooms/{c}/missions"), json!({ "goal": "Invoices", "lanes": lanes })).await;
    assert_eq!(st, 201, "{v}");
    let mission = v["id"].as_str().unwrap().to_string();
    steer(&s, "sam", &ws, &mission).await;
    // A task of sam's own, written after, that only mentions the same words.
    s.k.create_run("person:sam", "sam", "book the plum orchard picnic".into(), vec![], crate::kernel::NewRun::default()).await.unwrap();
    assert_eq!(tasks_holding(&s, "plum orchard").await, 3, "sam's lane task and steer do not hold mo's request");

    s.k.erase_person("person:sam", "mo").await.unwrap();

    assert_eq!(tasks_holding(&s, "plum orchard invoices").await, 0, "sam's lane task or steer still holds the erased requester's words");
    assert_eq!(tasks_holding(&s, "plum orchard picnic").await, 1, "sam's own task lost its words");
    s.done().await;
}

/// An `@Magnus` ask deleted for everyone left its words in the lane's task
/// and in each steer of the lane.
#[tokio::test(flavor = "multi_thread")]
async fn an_unsent_ask_leaves_its_lanes_tasks() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, c) = workspace(&s).await;
    let v = post(&s, "sam", &ws, &c, json!({ "text": "@Magnus summarise the plum orchard plan", "agent": true })).await;
    let mission = v["mission_id"].as_str().expect("the ask started no mission").to_string();
    steer(&s, "sam", &ws, &mission).await;
    assert_eq!(tasks_holding(&s, "plum orchard").await, 2, "the lane's task and its steer do not hold the ask");

    let sam = s.token("sam").await;
    let (st, r) = s.call(&sam, Method::DELETE, &format!("/v1/workspaces/{ws}/channels/{c}/messages/{}", v["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200, "{r}");

    assert_eq!(tasks_holding(&s, "plum orchard").await, 0, "a lane's task or steer still holds the deleted ask");
    s.done().await;
}
