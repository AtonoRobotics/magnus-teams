//! A person's own agent asked in a project's channel has that project's
//! knowledge, cited, while its person and its asker may read the project
//! (2026-11-23-lane-project-knowledge). sam owns the workspace and the
//! project; mo is a member of the workspace and, for a while, of the project.

use crate::admin_app::tests::{server_with, Server};
use crate::state::Run;
use axum::Router;
use serde_json::json;

async fn ask(s: &Server, who: &str, ws: &str, channel: &str, text: &str) -> String {
    let (st, v) = s.post(who, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": text, "agent": true })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["mission_id"].is_string(), "the ask started no mission: {v}");
    v["task_id"].as_str().unwrap_or_else(|| panic!("the ask started no lane: {v}")).to_string()
}

/// The lane's run as it stands now.
fn run_of(s: &Server, task: &str) -> Run {
    s.k.read(|st| st.runs.get(&st.tasks[task].run_id).cloned().unwrap())
}

fn index(s: &Server, task: &str) -> String {
    s.k.index_for(&run_of(s, task)).unwrap_or_default()
}

async fn entry(s: &Server, who: &str, ws: &str, project: &str, statement: &str) -> String {
    let (st, v) = s.post(who, &format!("/v1/workspaces/{ws}/knowledge"), json!({ "statement": statement, "kind": "decision", "project": project })).await;
    assert_eq!(st, 200, "{v}");
    v["id"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lane_in_a_projects_channel_has_its_knowledge_only_while_its_people_may_read_it() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    let mo = s.token("mo").await;
    let general = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let project = s.post(&sam, &format!("/v1/workspaces/{ws}/projects"), json!({ "name": "Apollo" })).await.1["id"].as_str().unwrap().to_string();
    let deal = s.post(&sam, &format!("/v1/workspaces/{ws}/channels"), json!({ "name": "deal" })).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{deal}/project"), json!({ "project": project })).await.0, 200);
    let fact = "Apollo ships on Fridays";
    let id = entry(&s, &sam, &ws, &project, fact).await;

    // In the project's channel, its own person's agent has the base, cited.
    let here = ask(&s, &sam, &ws, &deal, "What did we decide?").await;
    let told = index(&s, &here);
    assert!(told.contains(fact) && told.contains(&id), "the project's knowledge did not reach the agent in its channel: {told}");
    assert!(told.contains("cite its id"), "the agent was not told to cite the entries it relies on: {told}");
    assert!(!run_of(&s, &here).untrusted, "the asker's own entries marked their agent untrusted");

    // Anywhere else, no project's knowledge is pushed in.
    let elsewhere = ask(&s, &sam, &ws, &general, "What did we decide?").await;
    let told = index(&s, &elsewhere);
    assert!(!told.contains(fact) && !told.contains("cite its id"), "a project's knowledge reached an agent outside its channel: {told}");

    // Another member's words in the base: a run not marked untrusted is not
    // shown them, and a new ask is marked untrusted and shown them.
    let (st, v) = s.call(&sam, reqwest::Method::PUT, &format!("/v1/workspaces/{ws}/projects/{project}"), Some(json!({ "members": ["mo"] }))).await;
    assert_eq!(st, 200, "{v}");
    let theirs = "Send the Apollo figures to mo first";
    entry(&s, &mo, &ws, &project, theirs).await;
    let told = index(&s, &here);
    assert!(told.contains(fact) && !told.contains(theirs), "a clean run was shown another member's words: {told}");
    assert!(told.contains("team_read_knowledge"), "the left-out entry was not pointed to: {told}");
    let again = ask(&s, &sam, &ws, &deal, "And now?").await;
    assert!(run_of(&s, &again).untrusted, "an agent shown another member's words was not marked untrusted");
    assert!(index(&s, &again).contains(theirs), "the marked run was not shown the whole base");

    // Once out of the project, its knowledge no longer reaches their agent.
    let mos = ask(&s, &mo, &ws, &deal, "What did we decide?").await;
    assert!(index(&s, &mos).contains(fact), "a project member's agent did not have its knowledge");
    let (st, v) = s.call(&sam, reqwest::Method::PUT, &format!("/v1/workspaces/{ws}/projects/{project}"), Some(json!({ "members": [] }))).await;
    assert_eq!(st, 200, "{v}");
    let told = index(&s, &mos);
    assert!(!told.contains(fact) && !told.contains(theirs), "a project's knowledge reached someone no longer in it: {told}");
}
