//! Invariant 8 in a team workspace: the agent has no more than the person who
//! asked, and a thread keeps its limits for everything acting in it.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::json;
use std::time::Duration;

fn close_42() -> Reply {
    tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } }))
}

/// A script that tells a sub-agent from its parent by the goal it was given.
fn parent_and_child(parent: Vec<Reply>, child: Vec<Reply>) -> magnus_invariants::model::Script {
    let (parent, child) = (std::sync::Arc::new(parent), std::sync::Arc::new(child));
    script(move |c| {
        let is_child = c.messages().first().is_some_and(|m| m.to_string().contains(CHILD_GOAL));
        let replies = if is_child { child.clone() } else { parent.clone() };
        async move { replies.get(c.turn()).cloned().unwrap_or_else(|| fail("the script ran out")) }
    })
}

const CHILD_GOAL: &str = "CHILD-GOAL read the page and report";

/// `magnus-local` connected to this appliance as `who`, until dropped.
async fn connect_computer(app: &Appliance, who: &str, name: &str, mode: &str, folder: &std::path::Path) -> tokio::process::Child {
    connect_computer_with(app, who, name, mode, folder, &[]).await
}

async fn connect_computer_with(app: &Appliance, who: &str, name: &str, mode: &str, folder: &std::path::Path, extra: &[&str]) -> tokio::process::Child {
    let api = app.api(who);
    let child = tokio::process::Command::new(appliance::bin("magnus-local"))
        .args(["--server", &api.base, "--name", name, "--mode", mode, "--bwrap", "/usr/local/lib/magnus/bwrap"])
        .args(extra)
        .arg("--folder")
        .arg(folder)
        .env("MAGNUS_LOCAL_TOKEN", &api.token)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    eventually("the computer connected", PATIENCE, || async {
        let (_, v) = api.get("/v1/local/computers").await;
        v.as_array().and_then(|a| a.iter().find(|c| c["computer"]["name"] == name && c["online"] == true)).map(|_| ())
    })
    .await;
    child
}

// Step 10: the team workspace.

/// A workspace owned by alice, bob a member and carol a guest, with one channel.
async fn team(alice: &Api) -> (String, String) {
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Team" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    for (person, role) in [("bob", "member"), ("carol", "guest")] {
        let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": person, "role": role })).await;
        assert_eq!(code, 200, "{v}");
    }
    let (code, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "general" })).await;
    assert_eq!(code, 200, "{made}");
    (ws.clone(), made["id"].as_str().unwrap().to_string())
}

async fn ask_team(who: &Api, ws: &str, channel: &str, text: &str) -> (reqwest::StatusCode, serde_json::Value) {
    who.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": text, "agent": true })).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_team_request_runs_with_no_more_than_the_requesters_entitlements() {
    let crm = mcp::crm().start().await;
    let close = || vec![plan("close record 42"), close_42(), complete("done")];
    let model = FakeModel::start(steps(close())).await;
    let spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    assert!(alice.put_policy("org", &org_policy(&permit_execute("crm__update_record"))).await.is_success());
    let (ws, channel) = team(&alice).await;

    // A guest can talk, and cannot have the agent act.
    let before = model.requests().len();
    let (code, said) = ask_team(&carol, &ws, &channel, "Please close record 42.").await;
    assert_eq!(code, 403, "the team agent exceeded its requester: a guest's request was taken: {said}");
    let (code, _) = carol.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "Bob, please close record 42 for me." })).await;
    assert_eq!(code, 200, "a guest could not chat");
    assert_eq!(model.requests().len(), before, "the team agent exceeded its requester: a guest's words reached the model as a request");

    // A role the app was not granted to.
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;
    assert_eq!(code, 200, "{v}");
    let (code, made) = ask_team(&bob, &ws, &channel, "Close record 42.").await;
    assert_eq!(code, 200, "{made}");
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert_eq!(crm.effects("update_record"), 0, "the team agent exceeded its requester");
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("not open to the")), "the refusal did not reach the agent: {results:?}");
    let asked_by_bob = model.requests().into_iter().last().unwrap();
    assert!(asked_by_bob.text().contains("Earlier in this channel"), "the channel was not given as data");
    assert!(!asked_by_bob.offered_tools().iter().any(|t| t == "crm__update_record"), "the team agent exceeded its requester: it was offered the app");

    // The same request from someone whose role has it.
    model.set_script(steps(close()));
    let (_, made) = ask_team(&alice, &ws, &channel, "Close record 42.").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert_eq!(crm.effects("update_record"), 1, "the role that holds the app could not use it");

    // A file built from what only the requester may see stays with them
    // until they share it. A channel of their own, so this is the only taint.
    let (_, made) = alice.post(&format!("/v1/workspaces/{ws}/channels"), json!({ "name": "alice-only" })).await;
    let quiet = made["id"].as_str().unwrap().to_string();
    let write = tool("team_write_file", json!({ "name": "held.txt", "content": "from the record" }));
    model.set_script(steps(vec![plan("save"), tool("crm__get_record", json!({ "id": "42" })), write, complete("done")]));
    let (_, made) = ask_team(&alice, &ws, &quiet, "Save record 42 as a file.").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert_eq!(alice.get(&format!("/v1/workspaces/{ws}/files/held.txt")).await.0, 404, "data fetched under one member's entitlements reached the team volume");
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/held/held.txt/share"), json!({})).await;
    assert_eq!(code, 200, "the requester could not share it: {v}");
    assert_eq!(alice.get(&format!("/v1/workspaces/{ws}/files/held.txt")).await.0, 200, "the shared file is not on the team volume");

    // Asked through the workspace, a member's standing still counts: once
    // removed, they cannot ask.
    let (code, _) = alice.post(&format!("/v1/workspaces/{ws}/connectors"), json!({ "app": "crm", "roles": ["owner", "member"] })).await;
    assert_eq!(code, 200);
    let (code, _) = alice.delete(&format!("/v1/workspaces/{ws}/members/bob")).await;
    assert_eq!(code, 200);
    let (code, _) = ask_team(&bob, &ws, &channel, "Close record 42.").await;
    assert_eq!(code, 404, "the team agent exceeded its requester: a removed member's request was taken");
}

#[tokio::test(flavor = "multi_thread")]
async fn personal_context_and_credentials_never_serve_a_team_thread() {
    use base64::Engine;
    let memory = canary("personal-memory");
    let personal_file = canary("personal-file");
    let vault = canary("vault-entry");
    let note = canary("relationship-note");
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![plan("remember"), tool("memory_write", json!({ "fact": memory })), complete("done")])).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let task = alice.submit("Remember this.").await;
    alice.wait_settled(&task).await;
    let (_, listed) = alice.get("/v1/memory").await;
    assert!(listed.to_string().contains(&memory), "the memory was not written: {listed}");
    let (code, v) = alice.post("/v1/relationship", json!({ "action": "add", "text": note })).await;
    assert_eq!(code, 200, "{v}");
    assert!(alice.put_secret("alice_private", &vault).await.is_success());
    let data = base64::engine::general_purpose::STANDARD.encode(personal_file.as_bytes());
    let (code, made) = alice.post("/v1/files", json!({ "name": "mine.txt", "media_type": "text/plain", "data_b64": data })).await;
    assert!(code.is_success(), "{made}");
    let (ws, channel) = team(&alice).await;

    let send = || tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" }));
    let before = model.requests().len();
    model.set_script(steps(vec![plan("use what you can"), tool("memory_search", json!({ "query": "anything" })), send(), complete("done")]));
    let (code, made) = ask_team(&alice, &ws, &channel, "Use anything of mine that would help.").await;
    assert_eq!(code, 200, "{made}");
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    model.set_script(steps(vec![plan("send"), send(), complete("done")]));
    let (code, made) = ask_team(&bob, &ws, &channel, "Send the note from Alice's mail.").await;
    assert_eq!(code, 200, "{made}");
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;

    let requests = model.requests();
    let in_team = &requests[before..];
    assert!(!in_team.is_empty());
    for (what, secret) in [("memory", &memory), ("a relationship note", &note), ("a personal file", &personal_file), ("the vault", &vault)] {
        for r in in_team {
            assert!(!r.text().contains(secret.as_str()), "personal context reached a team thread: {what}");
        }
    }
    for r in in_team {
        for t in r.offered_tools() {
            assert!(
                !t.starts_with("memory_") && !t.starts_with("computer_") && !t.starts_with("browser_as_") && !t.starts_with("mail__"),
                "personal context reached a team thread: {t} was offered"
            );
        }
    }
    let results: Vec<String> = in_team.iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("personal to a member")), "a personal tool was not refused: {results:?}");
    assert!(results.iter().any(|t| t.contains("not granted to this workspace")), "a member's own app was not refused: {results:?}");
    assert_eq!(mail.effects("send"), 0, "personal context reached a team thread: a member's connector served it");
}

fn in_team(ws: &str, tail: &str) -> String {
    format!("/v1/workspaces/{ws}/{tail}")
}

/// A sub-agent in a team thread has its requester's scope, not more, and
/// what a tainted parent read travels with the goal it writes.
#[tokio::test(flavor = "multi_thread")]
async fn a_sub_agent_in_a_team_thread_keeps_the_threads_limits() {
    let crm = mcp::crm().start().await;
    let spawn = tool("start_sub_agent", json!({ "goal": CHILD_GOAL }));
    let write = tool("team_write_file", json!({ "name": "from-child.txt", "content": "built from the record" }));
    let model = FakeModel::start(parent_and_child(
        vec![plan("c"), spawn.clone(), complete("done")],
        vec![plan("c"), close_42(), tool("memory_search", json!({ "query": "x" })), complete("child done")],
    ))
    .await;
    let spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy(&permit_execute("crm__update_record"))).await.is_success());
    let (ws, channel) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;

    let (_, made) = ask_team(&bob, &ws, &channel, "Close it.").await;
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert_eq!(crm.effects("update_record"), 0, "a sub-agent exceeded its parent: it used an app its requester's role does not hold");
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("does not serve a team thread")), "a sub-agent reached the requester's personal things: {results:?}");

    // A channel of alice's own; the private read is the parent's, the write the child's.
    let (_, made) = alice.post(&in_team(&ws, "channels"), json!({ "name": "quiet" })).await;
    let quiet = made["id"].as_str().unwrap().to_string();
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;
    model.set_script(parent_and_child(vec![plan("c"), tool("crm__get_record", json!({ "id": "42" })), spawn, complete("done")], vec![plan("c"), write, complete("child done")]));
    let (_, made) = ask_team(&alice, &ws, &quiet, "Save it through a helper.").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert_eq!(alice.get(&in_team(&ws, "files/from-child.txt")).await.0, 404, "data fetched under one member's entitlements reached the team volume through a sub-agent");
}

/// Test 36a. An admin's ceiling only makes a person's own computer stricter.
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_ceiling_only_tightens_a_members_computer() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (ws, _) = team(&alice).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    let _computer = connect_computer(&app, "alice", "laptop", "allow", &path).await;
    let write = |n: &str| tool("local_write_file", json!({ "path": path.join(n), "content": "x" }));

    model.set_script(steps(vec![plan("w"), write("free.txt"), complete("done")]));
    let task = alice.submit("Write it.").await;
    alice.wait_settled(&task).await;
    assert!(path.join("free.txt").exists(), "allow did not allow, so this test shows nothing");

    let (code, v) = alice.post(&in_team(&ws, "local_ceiling"), json!({ "mode": "ask" })).await;
    assert_eq!(code, 200, "{v}");
    model.set_script(steps(vec![plan("w"), write("held.txt"), complete("done")]));
    let task = alice.submit("Write another.").await;
    alice.wait_item(&task, "approval").await;
    assert!(!path.join("held.txt").exists(), "a member's computer went past the workspace's ceiling");

    let (code, _) = app.api("bob").post(&in_team(&ws, "local_ceiling"), json!({ "mode": "allow" })).await;
    assert_eq!(code, 403, "a member lifted the workspace's ceiling");
}

/// Working notes are not facts, are seen in the thread and end with the task.
#[tokio::test(flavor = "multi_thread")]
async fn working_notes_are_scratch_and_end_with_their_task() {
    let crm = mcp::crm().start().await;
    let note = canary("working-note");
    let model = FakeModel::start(steps(vec![plan("n"), tool("team_note", json!({ "text": note })), close_42(), complete("done")])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "principal", &[("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (ws, channel) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;
    let (_, made) = ask_team(&alice, &ws, &channel, "Take a note, then close it.").await;
    let task = made["task_id"].as_str().unwrap().to_string();
    let item = alice.wait_item(&task, "approval").await;

    let notes = in_team(&ws, &format!("channels/{channel}/notes"));
    let (_, live) = bob.get(&notes).await;
    assert!(live.to_string().contains(&note), "a thread's members cannot see its working notes: {live}");
    let (_, one) = alice.get(&format!("/v1/workspaces/{ws}")).await;
    assert!(!one["knowledge"].to_string().contains(&note), "a working note was treated as fact");

    assert_eq!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await, 200);
    alice.wait_status(&task, &["completed"]).await;
    let (_, gone) = bob.get(&notes).await;
    assert!(!gone.to_string().contains(&note), "a working note outlived its task: {gone}");

    // The next request in the thread starts with no trace of it.
    model.set_script(steps(vec![plan("n"), complete("done")]));
    let (_, made) = ask_team(&bob, &ws, &channel, "Anything?").await;
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert!(!model.requests().into_iter().last().unwrap().text().contains(&note), "a note carried over between requests");
}

/// Test 37. A project's knowledge reaches its people and the agents acting
/// for them; retiring an entry stops it being read as current.
#[tokio::test(flavor = "multi_thread")]
async fn a_projects_knowledge_stays_with_its_people_and_stays_correct() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (ws, channel) = team(&alice).await;
    let (code, made) = alice.post(&in_team(&ws, "projects"), json!({ "name": "Apollo" })).await;
    assert_eq!(code, 200, "{made}");
    let project = made["id"].as_str().unwrap().to_string();
    let (code, v) = alice.post(&in_team(&ws, &format!("channels/{channel}/project")), json!({ "project": project })).await;
    assert_eq!(code, 200, "{v}");
    let (_, other) = alice.post(&in_team(&ws, "channels"), json!({ "name": "elsewhere" })).await;
    let other = other["id"].as_str().unwrap().to_string();

    let secret = canary("project-fact");
    let (code, made) = alice.post(&in_team(&ws, "knowledge"), json!({ "statement": secret, "kind": "decision", "project": project })).await;
    assert_eq!(code, 200, "{made}");
    let entry = made["id"].as_str().unwrap().to_string();

    // Bob is in the workspace and not the project.
    assert_eq!(bob.get(&in_team(&ws, &format!("knowledge?project={project}"))).await.0, 404, "another project's knowledge base was read by a non-member");
    let (_, bobs) = bob.get(&in_team(&ws, "knowledge/export")).await;
    assert!(!bobs.to_string().contains(&secret), "the export carried a project its reader is not in");
    let (code, _) = bob.post(&in_team(&ws, &format!("knowledge/{entry}/flag")), json!({ "why": "wrong" })).await;
    assert_eq!(code, 404, "someone outside the project reached its entry");

    model.set_script(steps(vec![plan("k"), tool("team_read_knowledge", json!({ "project": project })), complete("done")]));
    let (_, made) = ask_team(&bob, &ws, &other, "What do we know?").await;
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    let reached = model.requests().iter().any(|r| r.text().contains(&secret) || r.last_results().iter().any(|t| t.text.contains(&secret)));
    assert!(!reached, "the team agent read another project's knowledge base for someone not in it");

    // The agent working in the project's channel for a member has it, cited.
    model.set_script(steps(vec![plan("k"), complete("done")]));
    let (_, made) = ask_team(&alice, &ws, &channel, "What did we decide?").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    let asked = model.requests().into_iter().last().unwrap().text();
    assert!(asked.contains(&secret), "the project's knowledge did not reach its agent");
    assert!(asked.contains(&entry) && asked.contains("cite"), "the agent was not told to cite the entries it relies on");

    // What the agent learned from content outside a person says so.
    let write = tool("team_write_file", json!({ "name": "note.txt", "content": "a claim" }));
    let learned = canary("from-a-file");
    model.set_script(steps(vec![
        plan("k"),
        write,
        tool("team_read_file", json!({ "name": "note.txt" })),
        tool("team_add_fact", json!({ "text": learned, "workspace_level": true })),
        complete("done"),
    ]));
    let (_, made) = ask_team(&alice, &ws, &channel, "Learn it.").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    let (_, base) = alice.get(&in_team(&ws, "knowledge")).await;
    let added = base.as_array().unwrap().iter().find(|e| e["statement"] == json!(learned)).unwrap_or_else(|| panic!("{base}")).clone();
    assert_eq!(added["weight"], "outside", "an entry from outside a person was not marked: {added}");
    assert!(added["author"].as_str().unwrap().starts_with("agent:"), "{added}");

    // Only curators keep it right, and the retired entry is not read again.
    let (code, v) = alice.put(&in_team(&ws, &format!("projects/{project}")), json!({ "members": ["bob"] })).await;
    assert_eq!(code, 200, "{v}");
    let (code, _) = bob.post(&in_team(&ws, &format!("knowledge/{entry}/retire")), json!({})).await;
    assert_eq!(code, 403, "a member who is not a curator retired an entry");
    assert_eq!(bob.post(&in_team(&ws, &format!("knowledge/{entry}/flag")), json!({ "why": "looks stale" })).await.0, 200, "a member could not flag an entry");
    let (code, v) = alice.post(&in_team(&ws, &format!("knowledge/{entry}/retire")), json!({})).await;
    assert_eq!(code, 200, "{v}");
    model.set_script(steps(vec![plan("k"), complete("done")]));
    let (_, made) = ask_team(&alice, &ws, &channel, "And now?").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert!(!model.requests().into_iter().last().unwrap().text().contains(&secret), "a retired entry was read as current");

    // Deleting removes it from the record too.
    let (code, v) = alice.delete(&in_team(&ws, &format!("knowledge/{entry}"))).await;
    assert_eq!(code, 200, "{v}");
    assert!(alice.get(&in_team(&ws, &format!("knowledge?project={project}"))).await.1.as_array().unwrap().is_empty());
    assert!(!app.db.dump().await.contains(&secret), "a deleted entry's text is still in the database");
}

/// Test 38. A whisper is its owner's until they post it.
#[tokio::test(flavor = "multi_thread")]
async fn a_whisper_reaches_only_its_owner_until_they_post_it() {
    let said = canary("whispered");
    let model = FakeModel::start(steps(vec![plan("w"), complete(&said)])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let (ws, channel) = team(&alice).await;
    let line = canary("thread-line");
    bob.post(&in_team(&ws, &format!("channels/{channel}/messages")), json!({ "text": line })).await;
    let ask = canary("the-question");
    let (code, made) = alice.post(&in_team(&ws, &format!("channels/{channel}/messages")), json!({ "text": ask, "whisper": true })).await;
    assert_eq!(code, 200, "{made}");
    let task = made["task_id"].as_str().unwrap().to_string();
    alice.wait_status(&task, &["completed"]).await;
    assert!(model.requests().iter().any(|r| r.text().contains(&line)), "the whispering agent did not see the thread");

    for who in [&alice, &bob, &carol] {
        let (_, seen) = who.get(&in_team(&ws, &format!("channels/{channel}/messages"))).await;
        assert!(!seen.to_string().contains(&said) && !seen.to_string().contains(&ask), "a whisper reached the thread: {seen}");
    }
    let (_, bobs) = bob.get(&in_team(&ws, &format!("channels/{channel}/whispers"))).await;
    assert!(!bobs.to_string().contains(&said), "a whisper reached someone other than its owner");
    let (_, mine) = alice.get(&in_team(&ws, &format!("channels/{channel}/whispers"))).await;
    assert!(mine.to_string().contains(&said), "its owner did not get the reply: {mine}");

    assert_eq!(bob.post(&in_team(&ws, &format!("whispers/{task}/share")), json!({})).await.0, 404, "someone else posted a whisper");
    let (code, v) = alice.post(&in_team(&ws, &format!("whispers/{task}/share")), json!({})).await;
    assert_eq!(code, 200, "{v}");
    let (_, seen) = carol.get(&in_team(&ws, &format!("channels/{channel}/messages"))).await;
    assert!(seen.to_string().contains(&said), "posting it did not reach the thread");
}

/// Test 39. In a team thread an ask shows only to the person who asked, and
/// only they answer it (spec rev 140, How an ask looks): no other member of
/// the workspace can answer it, and it never appears in the thread. Named
/// approvers are retired
/// (decisions/2026-11-10-rewrites-reviewed-and-approvers-retired.md).
#[tokio::test(flavor = "multi_thread")]
async fn in_a_team_thread_only_the_person_who_asked_answers_an_ask() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![plan("c"), close_42(), complete("done")])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "principal", &[("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let (ws, channel) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner", "member"] })).await;
    let (_, made) = ask_team(&alice, &ws, &channel, "Close it.").await;
    let task = made["task_id"].as_str().unwrap().to_string();
    let item = alice.wait_item(&task, "approval").await;
    let id = item["id"].as_str().unwrap().to_string();
    assert_eq!(crm.effects("update_record"), 0);
    for (other, who) in [(&bob, "a member"), (&carol, "a guest")] {
        assert_ne!(other.respond(&id, json!({ "decision": "approve", "scope": "once" })).await, 200, "{who} answered someone else's ask");
        assert!(!other.open_items(&task, "approval").await.iter().any(|i| i["id"] == json!(id)), "an ask reached {who}, not the person who asked");
    }
    assert_eq!(crm.effects("update_record"), 0, "someone other than the person who asked approved");
    let (_, seen) = bob.get(&in_team(&ws, &format!("channels/{channel}/messages"))).await;
    assert!(!seen.to_string().contains(&id), "an ask appeared in the thread");
    assert_eq!(alice.respond(&id, json!({ "decision": "approve", "scope": "once" })).await, 200, "the person who asked could not answer");
    alice.wait_settled(&task).await;
    assert_eq!(crm.effects("update_record"), 1, "the answer of the person who asked did not count");
}

/// Test 40. The team agent's own work reports to the channel and has no one to ask.
#[tokio::test(flavor = "multi_thread")]
async fn the_team_agents_own_work_reports_to_the_channel_and_asks_no_one() {
    let crm = mcp::crm().start().await;
    let report = canary("routine-report");
    let model = FakeModel::start(steps(vec![plan("r"), close_42(), complete(&report)])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "principal", &[("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (ws, channel) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;
    let body = json!({ "channel": channel, "text": "Check the records.", "every_hours": 24 });
    assert_eq!(bob.post(&in_team(&ws, "routines"), body.clone()).await.0, 403, "a member set the team agent's standing work");
    let (code, v) = alice.post(&in_team(&ws, "routines"), body).await;
    assert_eq!(code, 200, "{v}");
    eventually("the routine reported", PATIENCE, || async {
        let (_, seen) = bob.get(&in_team(&ws, &format!("channels/{channel}/messages"))).await;
        seen.to_string().contains(&report).then_some(())
    })
    .await;
    assert_eq!(crm.effects("update_record"), 0, "the team agent's own work acted on something that would have asked");
    let (_, inbox) = alice.get("/v1/inbox").await;
    assert!(inbox["items"].as_array().unwrap().iter().all(|i| i["kind"] != "approval"), "background work asked someone: {inbox}");
    let calls = model.requests().len();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(model.requests().len(), calls, "a routine ran again inside its interval");
}

/// Test 41. A workspace computer: every member with the takeover permission
/// can open it, its agent uses it, and private data never reaches it.
#[tokio::test(flavor = "multi_thread")]
async fn the_team_computer_holds_only_what_every_member_may_see() {
    let computerd = Computerd::start();
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read")]);
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let (ws, _) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner", "member"] })).await;
    let shot = || tool("computer_screenshot", json!({}));

    let (code, _) = bob.post("/v1/computers", json!({ "workspace": ws })).await;
    assert_ne!(code, 200, "a member created the workspace's computer");
    let (code, made) = alice.post("/v1/computers", json!({ "workspace": ws })).await;
    assert_eq!(code, 200, "{made}");
    let id = made["id"].as_str().unwrap().to_string();
    let (code, v) = alice.post(&format!("/v1/computers/{id}/start"), json!({})).await;
    assert_eq!(code, 200, "{v}");

    // A guest has no takeover; a member does.
    assert_ne!(carol.post(&format!("/v1/computers/{id}/takeover"), json!({})).await.0, 200, "a guest took over the team computer");

    // Having read what only the requester may see, nothing reaches it.
    model.set_script(steps(vec![plan("s"), tool("crm__get_record", json!({ "id": "42" })), shot(), complete("done")]));
    let (_, made) = alice.post(&in_team(&ws, "channels"), json!({ "name": "quiet" })).await;
    let quiet = made["id"].as_str().unwrap().to_string();
    let (_, made) = ask_team(&alice, &ws, &quiet, "Read it, then look.").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("never reaches the team's computer")), "private data reached the team computer: {results:?}");

    // Deleting the workspace takes its computer.
    let (code, v) = alice.delete(&format!("/v1/workspaces/{ws}")).await;
    assert_eq!(code, 200, "{v}");
    let (_, listed) = alice.get("/v1/computers").await;
    assert!(
        listed["computers"].as_array().unwrap().iter().all(|c| c["id"] != json!(id) || c["status"] == "deleted" || c["status"] == "destroyed"),
        "the workspace's computer outlived it: {listed}"
    );
}

/// Test 42. The audit says who asked and what acted; a deleted workspace
/// leaves no words in the record.
#[tokio::test(flavor = "multi_thread")]
async fn the_audit_names_the_requester_and_deletion_removes_the_words() {
    let model = FakeModel::start(steps(vec![plan("a"), tool("team_post", json!({ "text": "hello" })), complete("done")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (ws, channel) = team(&alice).await;
    let line = canary("channel-line");
    bob.post(&in_team(&ws, &format!("channels/{channel}/messages")), json!({ "text": line })).await;
    let (_, made) = ask_team(&bob, &ws, &channel, "Say hello.").await;
    let task = made["task_id"].as_str().unwrap().to_string();
    bob.wait_settled(&task).await;

    assert_eq!(bob.get(&in_team(&ws, "audit")).await.0, 403, "a member read the workspace's audit");
    let (code, audit) = alice.get(&in_team(&ws, "audit")).await;
    assert_eq!(code, 200, "{audit}");
    let row = audit["runs"].as_array().unwrap().iter().find(|r| r["task"] == json!(task)).unwrap_or_else(|| panic!("{audit}")).clone();
    assert_eq!(row["requester"], "bob");
    assert_eq!(row["workspace"], json!(ws));
    assert!(row["agent"].as_str().unwrap().contains("team"));
    assert!(row["actions"].to_string().contains("team_post"), "{row}");
    assert!(audit["changes"].to_string().contains("person:bob"), "who changed what is missing: {audit}");

    assert!(app.db.dump().await.contains(&line), "the words should be recorded while the workspace lives");
    let (code, v) = alice.delete(&format!("/v1/workspaces/{ws}")).await;
    assert_eq!(code, 200, "{v}");
    assert!(!app.db.dump().await.contains(&line), "a deleted workspace's words are still in the database");
}
