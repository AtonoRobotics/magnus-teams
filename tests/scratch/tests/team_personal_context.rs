//! A team request is a team run from its first model call: it was once
//! recorded as one only after its run had been woken, so the first call
//! could go out as its requester's own, with their relationship notes in
//! the context and their memory tools offered (invariant 8, team).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::*;
use serde_json::json;

/// How many team requests are made: the race this guards against showed in
/// about one request of four.
const REQUESTS_COUNT: usize = 16;

#[tokio::test(flavor = "multi_thread")]
async fn a_team_request_is_never_its_requesters_own_even_at_its_first_call() {
    let note = canary("relationship-note");
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (code, v) = alice.post("/v1/relationship", json!({ "action": "add", "text": note })).await;
    assert_eq!(code, 200, "{v}");
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Team" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "general" })).await;
    let channel = made["id"].as_str().unwrap().to_string();
    for n in 0..REQUESTS_COUNT {
        model.set_script(steps(vec![contract_unchecked("answer", "answered"), complete("done")]));
        let (code, made) = alice.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": format!("TEAM-REQUEST-{n}"), "agent": true })).await;
        assert_eq!(code, 200, "{made}");
        alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    }
    let team: Vec<_> = model.requests().into_iter().filter(|r| r.body["messages"].to_string().contains("TEAM-REQUEST-")).collect();
    assert!(team.len() >= REQUESTS_COUNT, "the team requests did not reach the model: {}", team.len());
    for r in &team {
        assert!(!r.text().contains(note.as_str()), "a team run's call carried its requester's relationship note");
        let personal: Vec<String> = r.offered_tools().into_iter().filter(|t| t.starts_with("memory_")).collect();
        assert!(personal.is_empty(), "a team run's call was offered its requester's {personal:?}");
    }
}
