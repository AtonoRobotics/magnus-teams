//! A project is read only by its people (2026-11-23-project-lanes-file-no-memory):
//! a member's own agent asked in a project's channel files nothing into that
//! member's personal memory, where it would stay after they left the project.
//! The model here files every fact it is shown, so the memory pass, if it
//! ran, would keep the project's decision.

use magnus_invariants::model::Script;
use magnus_invariants::*;
use serde_json::{json, Value};

const PASS: &str = "You keep the person's long-term memory";
const FACT: &str = "Apollo launches on the ninth";

fn model() -> Script {
    script(|c| async move {
        if c.body["system"].to_string().contains(PASS) {
            return if c.text().contains(FACT) { tool("memory_write", json!({ "fact": format!("The person's project: {FACT}.") })) } else { Reply::Text(String::new()) };
        }
        match c.turn() {
            0 => plan("answer"),
            _ => complete(&format!("We decided: {FACT}.")),
        }
    })
}

async fn memories(api: &Api) -> Vec<Value> {
    api.get("/v1/memory").await.1["memory"].as_array().cloned().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lane_in_a_projects_channel_files_nothing_into_personal_memory() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (_, made) = alice.post("/v1/workspaces", json!({ "name": "Team" })).await;
    let ws = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": "bob", "role": "member" })).await.0, 200);
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/projects"), json!({ "name": "Apollo", "members": ["bob"] })).await;
    let project = made["id"].as_str().unwrap().to_string();
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "apollo" })).await;
    let channel = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/workspaces/{ws}/channels/{channel}/project"), json!({ "project": project })).await.0, 200);
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/knowledge"), json!({ "statement": FACT, "kind": "decision", "project": project })).await;
    assert_eq!(code, 200, "{v}");

    let (code, made) = bob.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "When do we launch?", "agent": true })).await;
    assert_eq!(code, 200, "{made}");
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    // The pass runs after the run ends; give it the time it takes.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Bob leaves the project; what his agent learned there must not stay with him.
    assert_eq!(alice.put(&format!("/v1/workspaces/{ws}/projects/{project}"), json!({ "members": [] })).await.0, 200);
    let kept = memories(&bob).await;
    assert!(!kept.iter().any(|f| f.to_string().contains(FACT)), "a project's knowledge was filed into a member's personal memory: {kept:?}");
    assert!(!m.requests().iter().any(|r| r.body["system"].to_string().contains(PASS)), "a lane in a project's channel ran the memory pass");
}
