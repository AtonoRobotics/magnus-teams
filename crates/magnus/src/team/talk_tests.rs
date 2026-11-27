//! A member's request in a team thread talks as any Magnus chat does. A
//! team run an older release recorded runs under the team chat prompt, is
//! offered no contract, plan or Done, and ends with its answer posted to
//! the thread; an ask now is the asker's own agent in a mission lane, on
//! the model they picked.

use crate::admin_app::tests::server_with;
use axum::Router;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_team_request_talks_like_chat_not_like_a_task() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.recorded_team_run(&ws, &channel, "sam", "what is on the launch page").await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());

    let kind = s.k.run_kind(&run.id);
    assert_eq!(kind, crate::run::kind::RunKind::TeamRequest, "a team request was not read as one");
    assert!(kind.converses() && kind.prompt() == crate::run::kind::Prompt::Team, "a team request was run as a task");
    assert_eq!(run.config.instructions, crate::run::team_instructions_version(), "a team request was not given the team chat prompt");
    assert!(!crate::run::TEAM_INSTRUCTIONS.contains("task_complete"), "the team prompt tells it to finish a task");
    let tools: Vec<String> = s.k.tools_for(&run).await.into_iter().map(|t| t.name).collect();
    for record in ["task_contract", "task_plan", "task_complete", "task_fail"] {
        assert!(!tools.contains(&record.to_string()), "a team request was offered {record}");
    }
    s.done().await;
}

/// Everything in the workspace is seen by the team (owner, 2026-11-13), and
/// a read of it reaches only the team. A member's private work goes to
/// their own agent.
#[tokio::test(flavor = "multi_thread")]
async fn posting_in_the_team_workspace_asks_no_one() {
    let s = server_with("", |_| Router::new()).await;
    for tool in ["team_read_knowledge", "web_fetch"] {
        assert!(!crate::boundary::facts(&s.k, tool).unwrap().reaches_self, "{tool} reaches only the team");
    }
    assert!(crate::run::TEAM_INSTRUCTIONS.contains("never ask the member to post"), "the team prompt lets it hand a post back to the member");
    s.done().await;
}

/// A team request runs on the model the member picked in the composer, as a
/// personal message does; a model no adapter offers is refused before
/// anything is posted.
#[tokio::test(flavor = "multi_thread")]
async fn a_team_request_runs_on_the_model_its_member_picked() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (_, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let path = format!("/v1/workspaces/{ws}/channels/{channel}/messages");
    let (st, v) = s.post(&sam, &path, json!({ "text": "plan it", "agent": true, "model": { "adapter": "nowhere", "model": "none" } })).await;
    assert_eq!(st, 400, "a model no adapter offers was taken: {v}");
    assert!(s.k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.is_empty()), "the refused message was posted");
    let (adapter, model) = s.k.default_model();
    let (st, v) = s.post(&sam, &path, json!({ "text": "plan it", "agent": true, "model": { "adapter": adapter, "model": model } })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v.get("mission_id").is_none(), "shared chat ask must not start a mission: {v}");
    assert!(v["task_id"].is_string(), "shared chat ask did not start a personal-agent reply: {v}");
    let run_id = v["run_id"].as_str().unwrap();
    let personal_run = s.k.read(|st| st.runs.get(run_id).cloned()).unwrap();
    assert_eq!(personal_run.person, "sam", "the reply did not run as the requester's personal agent");
    assert_eq!((personal_run.config.adapter, personal_run.config.model), (adapter, model));
    let picked = ("other".to_string(), "picked-model".to_string());
    let (_, _, picked_run_id) =
        s.k.start_personal_room_mission(&ws, &channel, "sam", "plan it", None, crate::say::Pick { model: Some(picked.clone()), reasoning: None }, false).await.unwrap();
    let picked_run = s.k.read(|st| st.runs.get(&picked_run_id).cloned().unwrap());
    assert_eq!((picked_run.config.adapter, picked_run.config.model), picked, "the picked model was not the personal lane's");
    s.done().await;
}

/// A lane whose run was stopped (its person's Stop, their removal) shares
/// nothing: it fails, and nothing is released into the room.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_lane_releases_nothing() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (_, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (mission, _, run) = s.k.start_personal_room_mission(&ws, &channel, "sam", "plan it", None, Default::default(), false).await.unwrap();
    s.k.emit("person:sam", crate::events::Ev::Cancelled { run: run.clone(), source: Some("button".into()), turn: None }).await.unwrap();
    for _ in 0..200 {
        let lane = s.k.read(|st| st.missions.mission(&mission).and_then(|m| m.lanes.values().next().cloned())).unwrap();
        assert_ne!(lane.status, crate::team::missions::LaneStatus::Released, "a stopped lane was released into the room");
        if lane.status == crate::team::missions::LaneStatus::Failed {
            assert_eq!(lane.failure_reason.as_deref(), Some("worker run was stopped"));
            return s.done().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the stopped lane never ended");
}

/// A `peer_agent` lane is refused before anything is recorded. Its dispatch
/// once recorded a lane that nothing would ever run.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_agent_lane_is_refused() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (_, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let lanes = json!([{ "kind": "peer_agent", "target_member": "sam", "objective": "Run the code agent on it" }]);
    let (st, v) = s.post(&sam, &format!("/v1/workspaces/{ws}/rooms/{channel}/missions"), json!({ "goal": "Fix it", "lanes": lanes })).await;
    assert_eq!(st, 400, "a peer_agent lane was taken: {v}");
    assert!(s.k.read(|st| st.missions.view_for_room(&ws, &channel, "sam").is_empty()), "the refused mission was recorded");
    s.done().await;
}
