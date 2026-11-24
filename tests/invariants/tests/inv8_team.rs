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
    // Text that is posted must not look like a secret: the scrubber holds any
    // 32-character letter-and-digit token, and a canary is one.
    let clean_summary = "Blockers: none this week.".to_string();
    let model = FakeModel::start(steps(vec![plan("summarize"), complete(&clean_summary)])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let (ws, channel) = team(&alice).await;

    // A guest can talk, and cannot have an agent act.
    let before = model.requests().len();
    let (code, said) = ask_team(&carol, &ws, &channel, "Please summarize.").await;
    assert_eq!(code, 403, "a guest's request was taken: {said}");
    let (code, _) = carol.post(&format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "Bob, please check." })).await;
    assert_eq!(code, 200, "a guest could not chat");
    assert_eq!(model.requests().len(), before, "a guest's words reached the model as a request");

    // A peer's @Magnus request dispatches their own personal Magnus via a Mission
    // lane, and clean output auto-releases inline into the shared conversation.
    let (code, made) = ask_team(&bob, &ws, &channel, "Summarize blockers.").await;
    assert_eq!(code, 200, "{made}");
    assert!(made["mission_id"].is_string(), "shared ask did not dispatch a personal-agent mission: {made}");
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;
    eventually("clean personal-agent summary posted to room", PATIENCE, || async {
        let (_, seen) = alice.get(&format!("/v1/workspaces/{ws}/channels/{channel}/messages")).await;
        seen.to_string().contains(&clean_summary).then_some(())
    })
    .await;

    // Output containing a credential pattern is held privately in the owner's
    // Release Airlock (invisible to teammates) until scrubbed and released.
    let leaked_key = "sk-secret123456789012345678901234567890";
    model.set_script(steps(vec![plan("draft"), complete(&format!("Key is {leaked_key}"))]));
    let (code, made) = ask_team(&alice, &ws, &channel, "Check my deployment note.").await;
    assert_eq!(code, 200, "{made}");
    let mission_id = made["mission_id"].as_str().unwrap().to_string();
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    let lane_id = eventually("scrub-flagged draft held in Alice's Release Airlock", PATIENCE, || async {
        let (_, ms) = alice.get(&format!("/v1/workspaces/{ws}/rooms/{channel}/missions")).await;
        ms.as_array()?
            .iter()
            .find(|m| m["id"] == json!(mission_id))?
            .get("lanes")?
            .as_array()?
            .iter()
            .find(|l| l["status"] == "waiting_release" && l["scrub_flagged"] == true)
            .and_then(|l| l["id"].as_str().map(String::from))
    })
    .await;
    let (_, bobs_view) = bob.get(&format!("/v1/workspaces/{ws}/rooms/{channel}/missions")).await;
    assert!(!bobs_view.to_string().contains(leaked_key), "flagged draft leaked to teammate: {bobs_view}");
    let (_, msgs) = bob.get(&format!("/v1/workspaces/{ws}/channels/{channel}/messages")).await;
    assert!(!msgs.to_string().contains(leaked_key), "flagged draft was posted to the shared room: {msgs}");

    // Scrubbing and releasing across the Airlock posts the clean update to the room.
    let scrubbed = "Deploy note, with the key taken out.";
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/missions/{mission_id}/lanes/{lane_id}/release"), json!({ "contribution": scrubbed })).await;
    assert_eq!(code, 200, "owner could not release scrubbed draft: {v}");
    let (_, msgs) = bob.get(&format!("/v1/workspaces/{ws}/channels/{channel}/messages")).await;
    assert!(msgs.to_string().contains(scrubbed), "released contribution did not reach the shared room: {msgs}");

    // Once removed, a former member cannot ask in the shared project.
    let (code, _) = alice.delete(&format!("/v1/workspaces/{ws}/members/bob")).await;
    assert_eq!(code, 200);
    let (code, _) = ask_team(&bob, &ws, &channel, "One more thing.").await;
    assert_eq!(code, 404, "a removed member's request was taken");
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

    // Bob's personal Magnus in the shared channel NEVER receives Alice's memory,
    // relationship notes, personal files, vault entries, or Alice's mail credentials.
    let before = model.requests().len();
    model.set_script(steps(vec![plan("search"), tool("memory_search", json!({ "query": "anything" })), complete("nothing found")]));
    let (code, made) = ask_team(&bob, &ws, &channel, "Check what Alice wrote in her private notes.").await;
    assert_eq!(code, 200, "{made}");
    bob.wait_settled(made["task_id"].as_str().unwrap()).await;

    let requests = model.requests();
    let bobs_runs = &requests[before..];
    assert!(!bobs_runs.is_empty());
    for (what, secret) in [("memory", &memory), ("a relationship note", &note), ("a personal file", &personal_file), ("the vault", &vault)] {
        for r in bobs_runs {
            assert!(!r.text().contains(secret.as_str()), "Alice's personal context reached Bob's run: {what}");
        }
    }
    assert_eq!(mail.effects("send"), 0, "Alice's connector acted on Bob's shared request");

    // A cross-user Mission lane targeting Alice holds its output privately in Alice's
    // Release Airlock; Bob never sees Alice's unreleased draft.
    let private_draft = canary("alice-private-draft");
    model.set_script(steps(vec![plan("check"), complete(&private_draft)]));
    let (code, m) = bob
        .post(
            &in_team(&ws, &format!("rooms/{channel}/missions")),
            json!({ "goal": "Gather status", "lanes": [{ "kind": "member_agent", "target_member": "alice", "objective": "Share status", "scope": "status" }] }),
        )
        .await;
    assert_eq!(code, 201, "{m}");
    eventually("cross-user draft held in Alice's Airlock", PATIENCE, || async {
        let (_, view) = alice.get(&in_team(&ws, &format!("rooms/{channel}/missions"))).await;
        view.to_string().contains(&private_draft).then_some(())
    })
    .await;
    let (_, bobs_missions) = bob.get(&in_team(&ws, &format!("rooms/{channel}/missions"))).await;
    assert!(!bobs_missions.to_string().contains(&private_draft), "Alice's unreleased draft leaked to Bob: {bobs_missions}");
}

fn in_team(ws: &str, tail: &str) -> String {
    format!("/v1/workspaces/{ws}/{tail}")
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
    // Its own calls, not only the last one: a person's agent files memory
    // after its run, in a call of its own.
    model.set_script(steps(vec![plan("k"), complete("done")]));
    let before = model.requests().len();
    let (_, made) = ask_team(&alice, &ws, &channel, "What did we decide?").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    let asked = model.requests()[before..].iter().map(|r| r.text()).collect::<Vec<_>>().join("\n");
    assert!(asked.contains(&secret), "the project's knowledge did not reach its agent");
    assert!(asked.contains(&entry) && asked.contains("cite"), "the agent was not told to cite the entries it relies on");

    // Only curators keep it right, and the retired entry is not read again.
    let (code, v) = alice.put(&in_team(&ws, &format!("projects/{project}")), json!({ "members": ["bob"] })).await;
    assert_eq!(code, 200, "{v}");
    let (code, _) = bob.post(&in_team(&ws, &format!("knowledge/{entry}/retire")), json!({})).await;
    assert_eq!(code, 403, "a member who is not a curator retired an entry");
    assert_eq!(bob.post(&in_team(&ws, &format!("knowledge/{entry}/flag")), json!({ "why": "looks stale" })).await.0, 200, "a member could not flag an entry");
    let (code, v) = alice.post(&in_team(&ws, &format!("knowledge/{entry}/retire")), json!({})).await;
    assert_eq!(code, 200, "{v}");
    model.set_script(steps(vec![plan("k"), complete("done")]));
    let before = model.requests().len();
    let (_, made) = ask_team(&alice, &ws, &channel, "And now?").await;
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;
    assert!(model.requests().len() > before, "the ask reached no model");
    assert!(!model.requests()[before..].iter().any(|r| r.text().contains(&secret)), "a retired entry was read as current");

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
    // Posted text, so not secret-shaped (the scrubber would hold a canary).
    let report = "Routine report: the records are in order.".to_string();
    let model = FakeModel::start(steps(vec![plan("r"), close_42(), complete(&report)])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "principal", &[("update_record", "irreversible")]);
    let app = Appliance::start(spec, &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (ws, channel) = team(&alice).await;
    alice.post(&in_team(&ws, "connectors"), json!({ "app": "crm", "roles": ["owner"] })).await;
    let body = json!({ "channel": channel, "text": "Check the records.", "every_hours": 24 });
    assert_eq!(app.api("carol").post(&in_team(&ws, "routines"), body.clone()).await.0, 403, "a guest set a shared routine");
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

/// Test 41. Shared projects do not auto-provision a shared computer VM: each
/// collaborator's personal Magnus runs on their own isolated personal computer.
#[tokio::test(flavor = "multi_thread")]
async fn shared_projects_do_not_spawn_a_shared_computer_vm() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (ws, _) = team(&alice).await;
    let (_, listed) = alice.get("/v1/computers").await;
    let ws_computers: Vec<_> = listed["computers"].as_array().into_iter().flatten().filter(|c| c["owner"]["workspace"] == json!(ws)).collect();
    assert!(ws_computers.is_empty(), "creating a shared project auto-provisioned a shared computer VM: {ws_computers:?}");
}

/// Test 42. The audit says who asked and what acted; a deleted workspace
/// leaves no words in the record.
#[tokio::test(flavor = "multi_thread")]
async fn the_audit_names_the_requester_and_deletion_removes_the_words() {
    let reply = canary("hello-shared");
    let model = FakeModel::start(steps(vec![plan("a"), complete(&reply)])).await;
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
    assert!(audit["changes"].to_string().contains("person:bob"), "who changed what is missing: {audit}");

    assert!(app.db.dump().await.contains(&line), "the words should be recorded while the workspace lives");
    let (code, v) = alice.delete(&format!("/v1/workspaces/{ws}")).await;
    assert_eq!(code, 200, "{v}");
    assert!(!app.db.dump().await.contains(&line), "a deleted workspace's words are still in the database");
}
