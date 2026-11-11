//! A workspace owner sets monthly spend limits for the workspace and for a
//! channel; once one is reached, a mention posts the message but starts no
//! work, and only an owner can set them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_limit_keeps_the_team_agent_from_starting() {
    let model = FakeModel::start(steps(vec![complete("done")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (_, made) = alice.post("/v1/workspaces", json!({ "name": "Team" })).await;
    let ws = made["id"].as_str().unwrap().to_string();
    let (code, _) = alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": "bob", "role": "admin" })).await;
    assert_eq!(code, 200);
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "costly" })).await;
    let costly = made["id"].as_str().unwrap().to_string();
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "cheap" })).await;
    let cheap = made["id"].as_str().unwrap().to_string();

    let (code, v) = bob.post(&format!("/v1/workspaces/{ws}/limit"), json!({ "usd": 5.0 })).await;
    assert_eq!(code, 403, "an admin who is not an owner set a spend limit: {v}");
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/limit"), json!({ "usd": -1.0 })).await;
    assert_eq!(code, 400, "a negative limit was taken: {v}");
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/limit"), json!({ "channel": costly, "usd": 0.0 })).await;
    assert_eq!(code, 200, "{v}");

    let (code, v) = bob.post(&format!("/v1/workspaces/{ws}/channels/{costly}/messages"), json!({ "text": "@Magnus do the costly thing" })).await;
    assert_eq!(code, 200, "{v}");
    assert!(v["id"].is_string(), "the message was not posted: {v}");
    assert!(v["task_id"].is_null(), "the agent started past the channel's limit: {v}");
    assert!(v["limited"].as_str().unwrap_or_default().contains("This channel"), "the poster is not told why: {v}");

    let (_, v) = bob.post(&format!("/v1/workspaces/{ws}/channels/{cheap}/messages"), json!({ "text": "@Magnus do the cheap thing" })).await;
    assert!(v["task_id"].is_string(), "another channel's limit stopped work here: {v}");
    bob.wait_settled(v["task_id"].as_str().unwrap()).await;

    let (_, home) = bob.get(&format!("/v1/workspaces/{ws}/home")).await;
    assert_eq!(home["spend"]["channels"][&costly]["limit_usd"], 0.0, "the channel's limit is not shown: {home}");
    assert!(home["spend"]["limit_usd"].is_null());

    let (code, _) = alice.post(&format!("/v1/workspaces/{ws}/limit"), json!({ "usd": 0.0 })).await;
    assert_eq!(code, 200);
    let (_, v) = bob.post(&format!("/v1/workspaces/{ws}/channels/{cheap}/messages"), json!({ "text": "@Magnus once more" })).await;
    assert!(v["task_id"].is_null(), "the agent started past the workspace's limit: {v}");
    assert!(v["limited"].as_str().unwrap_or_default().contains("This workspace"), "{v}");

    let (code, _) = alice.post(&format!("/v1/workspaces/{ws}/limit"), json!({})).await;
    assert_eq!(code, 200);
    model.set_script(steps(vec![complete("done")]));
    let (_, v) = bob.post(&format!("/v1/workspaces/{ws}/channels/{cheap}/messages"), json!({ "text": "@Magnus and now" })).await;
    assert!(v["task_id"].is_string(), "lifting the workspace's limit did not let work start: {v}");
}
