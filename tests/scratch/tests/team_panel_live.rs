//! The right panel answers while the team agent is at work. It once looked
//! up the requester's name while holding the state's lock, which took the
//! same lock again: the first look at a project with work running hung the
//! whole server.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::*;
use serde_json::json;
use std::time::Duration;

/// How long the model holds its answer, so the run is at work meanwhile.
const MODEL_HOLD_SECS: u64 = 20;
/// How long one look at the panel may take before it counts as hung.
const PANEL_ANSWER_SECS: u64 = 5;
/// How long to wait for the run to show in the panel at all.
const WORK_SHOWS_SECS: u64 = 15;

#[tokio::test(flavor = "multi_thread")]
async fn the_panel_answers_while_the_team_agent_works() {
    let model = FakeModel::start(script(|_| async {
        tokio::time::sleep(Duration::from_secs(MODEL_HOLD_SECS)).await;
        complete("done")
    }))
    .await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Team" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "work" })).await;
    let channel = made["id"].as_str().unwrap().to_string();
    let (code, made) = alice.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "@Magnus look into it" })).await;
    assert_eq!(code, 200, "{made}");
    assert!(made["task_id"].is_string(), "the mention did not start the agent: {made}");

    let end = std::time::Instant::now() + Duration::from_secs(WORK_SHOWS_SECS);
    loop {
        let look = tokio::time::timeout(Duration::from_secs(PANEL_ANSWER_SECS), alice.get(&format!("/v1/workspaces/{ws}/panel"))).await;
        let (code, panel) = look.expect("the panel hung while the team agent was at work");
        assert_eq!(code, 200, "{panel}");
        if let Some(w) = panel["work"].as_array().and_then(|w| w.first()) {
            assert_eq!(w["requester"], "alice");
            assert!(!w["requester_name"].as_str().unwrap_or_default().is_empty(), "the panel does not name who asked: {w}");
            break;
        }
        assert!(std::time::Instant::now() < end, "the agent's work never showed in the panel: {panel}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // The server still answers anything else.
    let (code, _) = tokio::time::timeout(Duration::from_secs(PANEL_ANSWER_SECS), alice.get("/v1/workspaces")).await.expect("the server hung");
    assert_eq!(code, 200);
}
