//! A member's request in a team thread talks as any Magnus chat does: it
//! runs under the team chat prompt, is offered no contract, plan or Done,
//! and ends with its answer posted to the thread. A sub-agent it starts
//! is a task like any sub-agent.

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
    let (task, _) = s.k.team_request(&ws, &channel, None, "sam", "what is on the launch page".into()).await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());

    assert!(s.k.team_talks(&run) && s.k.converses(&run), "a team request was run as a task");
    assert_eq!(run.config.instructions, crate::run::team_instructions_version(), "a team request was not given the team chat prompt");
    assert!(!crate::run::TEAM_INSTRUCTIONS.contains("task_complete"), "the team prompt tells it to finish a task");
    let tools: Vec<String> = s.k.tools_for(&run).await.into_iter().map(|t| t.name).collect();
    for record in ["task_contract", "task_plan", "task_complete", "task_fail"] {
        assert!(!tools.contains(&record.to_string()), "a team request was offered {record}");
    }
    s.done().await;
}

/// Everything in the workspace is seen by the team (owner, 2026-11-13): a
/// post, a file or a fact there is not a send, so it never asks the member
/// first. A member's private work goes to their own agent.
#[tokio::test(flavor = "multi_thread")]
async fn posting_in_the_team_workspace_asks_no_one() {
    let s = server_with("", |_| Router::new()).await;
    for tool in ["team_post", "team_write_file", "team_note", "team_add_fact", "team_fix_entry", "team_create"] {
        let f = crate::boundary::facts(&s.k, tool).unwrap();
        assert!(f.reaches_self, "{tool} is treated as a send out of the workspace");
    }
    for tool in ["team_read_file", "team_read_knowledge", "web_fetch"] {
        assert!(!crate::boundary::facts(&s.k, tool).unwrap().reaches_self, "{tool} reaches only the team");
    }
    assert!(crate::run::TEAM_INSTRUCTIONS.contains("never ask the member to post"), "the team prompt lets it hand a post back to the member");
    s.done().await;
}
