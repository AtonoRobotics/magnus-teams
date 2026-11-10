//! Invariant 8: policy is layered, organization down to user. The agent never
//! exceeds the rights of the person it acts for.
//!
//! Prevents: an agent with more access than the person who asked, and a lower
//! layer quietly widening what the organization allows.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::json;
use std::time::Duration;

fn close_42() -> Reply {
    tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } }))
}

fn crm_script() -> magnus_invariants::model::Script {
    steps(vec![contract_unchecked("close record 42", "closed"), close_42(), fail("done")])
}

async fn crm_app(model: &FakeModel) -> (FakeMcp, Appliance) {
    let crm = mcp::crm().start().await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]), model).await;
    (crm, app)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_layer_cannot_widen_the_organization_layer() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("say hi", "said"),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(bob.put_policy("user/bob", &permit_execute("mail__send")).await.is_success());

    let id = bob.submit("Email bob hello.").await;
    bob.wait_item(&id, "approval").await;
    assert_eq!(mail.effects("send"), 0, "a user-layer permit went past the organization's ceiling");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_organization_forbid_wins_over_any_lower_permit() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let org = org_policy(r#"forbid(principal, action, resource == Tool::"crm__update_record");"#);
    assert!(alice.put_policy("org", &org).await.is_success());
    assert!(bob.put_policy("user/bob", &permit_execute("crm__update_record")).await.is_success());
    let id = bob.submit("Close CRM record 42.").await;
    bob.wait_settled(&id).await;
    assert_eq!(crm.effects("update_record"), 0);
    assert!(bob.open_items(&id, "approval").await.is_empty(), "a forbidden action was offered for approval");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_layer_narrows_for_that_user_only() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(bob.put_policy("user/bob", r#"forbid(principal, action == Action::"execute", resource == Tool::"crm__update_record");"#).await.is_success());

    let b = bob.submit("Close CRM record 42.").await;
    bob.wait_settled(&b).await;
    assert_eq!(crm.effects("update_record"), 0, "bob's own narrowing was ignored");

    let a = alice.submit("Close CRM record 42.").await;
    alice.wait_settled(&a).await;
    assert_eq!(crm.effects("update_record"), 1, "bob's layer narrowed alice");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_layer_narrows_its_members() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(alice.put_policy("group/finance", r#"forbid(principal, action, resource == Tool::"crm__update_record");"#).await.is_success());

    let b = bob.submit("Close CRM record 42.").await; // bob is in finance
    bob.wait_settled(&b).await;
    assert_eq!(crm.effects("update_record"), 0, "the finance group's layer did not apply to bob");

    let a = alice.submit("Close CRM record 42.").await; // alice is not
    alice.wait_settled(&a).await;
    assert_eq!(crm.effects("update_record"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_never_exceeds_the_persons_own_access() {
    let finance = mcp::finance().start().await;
    let model =
        FakeModel::start(steps(vec![contract_unchecked("get the Q3 report", "report fetched"), tool("finance__get_report", json!({ "quarter": "Q3" })), fail("done")])).await;
    let app = Appliance::start(Spec::default().app("finance", &finance, "internal", "private", &[("get_report", "read")]).access("finance", &["finance"]), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    // The organization lets the agent do anything at all.
    assert!(alice.put_policy("org", "permit(principal, action, resource);").await.is_success());

    let a = alice.submit("Get me the Q3 finance report.").await; // alice is not in finance
    alice.wait_settled(&a).await;
    assert!(finance.calls().is_empty(), "the agent used access alice does not have");
    assert!(alice.open_items(&a, "approval").await.is_empty(), "an approval was offered to get around alice's rights");

    let b = bob.submit("Get me the Q3 finance report.").await; // bob is
    bob.wait_settled(&b).await;
    assert_eq!(finance.calls_to("get_report").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_holders_of_the_approver_role_can_approve() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("say hi", "said"),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let (bob, carol) = (app.api("bob"), app.api("carol"));
    let id = carol.submit("Email bob hello.").await;
    let item = carol.wait_item(&id, "approval").await;
    let iid = item["id"].as_str().unwrap();

    let st = carol.respond(iid, json!({ "decision": "approve", "scope": "once" })).await;
    assert!(st == 403 || st == 404, "carol approved without the approver role: {st}");
    never("an approval without the role took effect", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;

    assert!(bob.respond(iid, json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("bob's approval took effect", PATIENCE, || async { (mail.effects("send") == 1).then_some(()) }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_persons_computer_cannot_reach_anothers() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let site = FakeWeb::start(&[("/ok", "<html>bob's destination</html>")]).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "bobs.test": [site.base.trim_start_matches("http://")] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let start = |who: &'_ Api| {
        let who = who.clone();
        async move {
            let (_, made) = who.post("/v1/computers", json!({})).await;
            let id = made["id"].as_str().unwrap().to_string();
            let (code, started) = who.post(&format!("/v1/computers/{id}/start"), json!({})).await;
            assert_eq!(code, 200, "{started}");
            id
        }
    };
    let (alices, bobs) = (start(&alice).await, start(&bob).await);
    let (code, _) = bob.post(&format!("/v1/computers/{bobs}/destinations"), json!({ "host": "bobs.test" })).await;
    assert_eq!(code, 200);
    let marker = canary("bobs-computer");
    computerd.exec(&bobs, &format!("echo {marker} > /workspace/marker; python3 -m http.server 8000 --directory /workspace >/dev/null 2>&1 &"));

    // Every computer has the same private addresses, on its own network.
    let tried = computerd.root_shell(
        &alices,
        "ip route add default via 10.200.0.1 dev eth0; for t in 10.200.0.2:8000 10.200.0.1:8000 10.200.0.6:8000; do curl -s -m 3 http://$t/marker; done; echo finished",
    );
    assert!(tried.contains("finished"), "the test's script did not run: {tried}");
    assert!(!tried.contains(&marker), "Alice's computer reached Bob's VM: {tried}");

    // Asking the proxy in Bob's name does nothing: the socket says who it is.
    let code = computerd.root_shell(&alices, "curl -s -m 20 -o /dev/null -w '%{http_code}' -x http://10.200.0.1:3128 -H 'X-Computer: bob' http://bobs.test/ok");
    assert_eq!(code, "403", "Alice's computer reached Bob's destination through the proxy");
    assert!(site.hits().is_empty(), "Alice's computer reached Bob's proxy path");

    // Each computer's own CA, so one's interception certificate means nothing on the other.
    let alice_ca = computerd.exec(&alices, "cat /usr/local/share/ca-certificates/magnus-computer.crt");
    let bob_ca = computerd.exec(&bobs, "cat /usr/local/share/ca-certificates/magnus-computer.crt");
    assert!(alice_ca.contains("BEGIN CERTIFICATE") && bob_ca.contains("BEGIN CERTIFICATE"), "a computer has no CA");
    assert_ne!(alice_ca, bob_ca, "Alice's computer shares a CA with Bob's");
}

/// Test 29. An operator reads a computer only in a session its user granted,
/// and the user sees that session.
#[tokio::test(flavor = "multi_thread")]
async fn operator_routes_reach_a_computer_only_in_a_session_its_user_granted() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let mut spec = Spec::default().computer(&computerd.socket);
    spec.people[0].roles.push("developer".to_string());
    let app = Appliance::start(spec, &model).await;
    let (operator, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let start = |who: Api| async move {
        let (_, made) = who.post("/v1/computers", json!({})).await;
        let id = made["id"].as_str().unwrap().to_string();
        let (code, started) = who.post(&format!("/v1/computers/{id}/start"), json!({})).await;
        assert_eq!(code, 200, "{started}");
        id
    };
    let (id, other) = (start(bob.clone()).await, start(bob.clone()).await);
    let secret = canary("bobs-file");
    computerd.exec(&id, &format!("echo {secret} > /workspace/note"));
    let file = format!("/v1/operator/computers/{id}/files?path=/workspace/note");
    let screen = format!("/v1/operator/computers/{id}/screen");

    let refused = |who: Api, path: String| async move {
        let (code, body) = who.get(&path).await;
        assert!(code.is_client_error(), "an operator read a computer without the user's grant: {path} {code} {body}");
    };
    // The ordinary routes refuse an owner of the appliance too.
    refused(operator.clone(), format!("/v1/computers/{id}/files?path=/workspace/note")).await;
    refused(operator.clone(), file.clone()).await;
    refused(operator.clone(), screen.clone()).await;

    // A grant to someone else opens nothing for this operator.
    let (code, body) = bob.post(&format!("/v1/computers/{id}/operator-sessions"), json!({ "operator": "carol", "minutes": 30 })).await;
    assert_eq!(code, 200, "{body}");
    refused(operator.clone(), file.clone()).await;
    // Nor can anyone but its user grant one.
    let (code, _) = carol.post(&format!("/v1/computers/{id}/operator-sessions"), json!({ "operator": "alice", "minutes": 30 })).await;
    assert!(code.is_client_error(), "someone else granted a session on this computer");

    let (code, body) = bob.post(&format!("/v1/computers/{id}/operator-sessions"), json!({ "operator": "alice", "minutes": 30 })).await;
    assert_eq!(code, 200, "{body}");
    let session = body["session"].as_str().unwrap().to_string();
    let (code, read) = operator.get(&file).await;
    assert_eq!(code, 200, "{read}");
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, read["data_b64"].as_str().unwrap()).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains(&secret), "the granted route did not serve the file");
    let (code, _) = operator.get(&screen).await;
    assert_eq!(code, 200, "the granted route did not serve the screen");
    refused(operator.clone(), format!("/v1/operator/computers/{other}/files?path=/workspace/note")).await;

    let (_, activity) = bob.get(&format!("/v1/computers/{id}/activity")).await;
    let text = activity.to_string();
    assert!(text.contains("alice") && text.contains("the file /workspace/note") && text.contains("\"at\""), "a granted session is missing from the user's log: {text}");

    let (code, _) = bob.delete(&format!("/v1/computers/{id}/operator-sessions/{session}")).await;
    assert!(code.is_success());
    refused(operator.clone(), file.clone()).await;

    let (code, body) = bob.post(&format!("/v1/computers/{id}/operator-sessions"), json!({ "operator": "alice", "minutes": 1 })).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(operator.get(&file).await.0, 200);
    tokio::time::sleep(Duration::from_secs(61)).await;
    refused(operator.clone(), file.clone()).await;

    let (code, audit) = operator.get("/v1/dev/runs").await;
    assert!(code.is_success(), "observability needs no grant");
    assert!(!audit.to_string().contains(&secret), "an observability route carries file content");
}

// Step 9: a sub-agent and the person's own computer.

const CHILD_GOAL: &str = "CHILD-GOAL read the page and report";

/// A script that tells a sub-agent from its parent by the goal it was given.
fn parent_and_child(parent: Vec<Reply>, child: Vec<Reply>) -> magnus_invariants::model::Script {
    let (parent, child) = (std::sync::Arc::new(parent), std::sync::Arc::new(child));
    script(move |c| {
        let is_child = c.messages().first().is_some_and(|m| m.to_string().contains(CHILD_GOAL));
        let replies = if is_child { child.clone() } else { parent.clone() };
        async move { replies.get(c.turn()).cloned().unwrap_or_else(|| fail("the script ran out")) }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sub_agent_never_holds_more_than_its_parent() {
    let site = FakeWeb::start(&[("/p", "<html><p>words from outside</p></html>")]).await;
    let mail = mcp::mail().start().await;
    // A sub-agent works under its parent's grant and no other: a grant named
    // when it starts is read as nothing (decisions/2026-11-10-sub-agents-carry-parent-authority.md).
    let parent = vec![
        contract_unchecked("delegate", "done"),
        tool("start_sub_agent", json!({ "goal": CHILD_GOAL, "tools": ["task_contract", "web_fetch", "task_complete"], "grant": "not-a-grant-this-run-holds" })),
        complete("done"),
    ];
    let child = vec![
        contract_unchecked("read", "read"),
        tool("web_fetch", json!({ "url": site.url("/p") })),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        complete("the child's report"),
    ];
    let model = FakeModel::start(parent_and_child(parent, child)).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let alice = app.api("alice");
    let policy = [permit_execute("web_fetch"), permit_execute("mail__send")].join("\n");
    assert!(alice.put_policy("org", &org_policy(&policy)).await.is_success());
    let task = alice.submit("Delegate this.").await;
    alice.wait_settled(&task).await;

    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("was not given mail__send")), "a sub-agent exceeded its parent: it used a tool it was not given: {results:?}");
    assert_eq!(mail.effects("send"), 0, "a sub-agent exceeded its parent");
    assert_eq!(alice.task(&task).await["taint"]["untrusted"], json!(true), "a sub-agent's reading did not taint its parent");

    let bodies: Vec<serde_json::Value> =
        app.db.admin().await.query("SELECT body FROM public.events", &[]).await.unwrap().iter().map(|r| r.get::<_, serde_json::Value>(0)).collect();
    let child_run = bodies.iter().find(|b| b["child"].is_string() && b["parent"].is_string()).expect("the sub-agent was not recorded")["child"].as_str().unwrap().to_string();
    let created = bodies.iter().find(|b| b["run"] == child_run.as_str() && b["person"].is_string()).expect("the child run was not recorded");
    assert_eq!(created["person"], "alice", "a sub-agent exceeded its parent: it acted as someone else");
    assert!(!created["grant"].is_string(), "a sub-agent exceeded its parent: it holds a grant the parent lacks: {created}");
    assert!(bodies.iter().any(|b| b["run"] == child_run.as_str() && b["tool"] == "web_fetch"), "the child's action does not carry the child's run id");
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

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_cannot_touch_a_desktop_or_folder_not_connected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (granted, outside) = (root.join("granted"), root.join("outside"));
    std::fs::create_dir_all(&granted).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let secret = canary("outside-the-folder");
    std::fs::write(outside.join("secret"), &secret).unwrap();
    std::fs::write(granted.join("ok.txt"), "hello from the granted folder").unwrap();
    std::os::unix::fs::symlink(outside.join("secret"), granted.join("link")).unwrap();
    let read = |p: std::path::PathBuf| tool("local_read_file", json!({ "path": p }));

    // No connection: nothing runs.
    let model = FakeModel::start(steps(vec![contract_unchecked("read", "read"), read(outside.join("secret")), complete("done")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Read that file.").await;
    alice.wait_settled(&task).await;
    assert!(model.requests().iter().flat_map(|r| r.last_results()).any(|r| r.text.contains("no computer is connected")), "the agent reached a computer that was not connected");
    assert!(!model.requests().iter().any(|r| r.text().contains(&secret)), "the agent reached outside a granted folder");

    // Connected: only the granted folder, whatever the path says.
    let mut replies = vec![contract_unchecked("read", "read"), read(granted.join("ok.txt")), read(outside.join("secret")), read(granted.join("link"))];
    replies.push(tool("local_exec", json!({ "command": format!("cat {}", outside.join("secret").display()), "cwd": granted })));
    replies.push(complete("done"));
    model.set_script(steps(replies));
    let _computer = connect_computer(&app, "alice", "laptop", "allow", &granted).await;
    assert!(alice.put_policy("org", &org_policy(&permit_execute("local_exec"))).await.is_success());
    let task = alice.submit("Read those files.").await;
    alice.wait_settled(&task).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("hello from the granted folder")), "a granted file did not arrive: {results:?}");
    assert!(!model.requests().iter().any(|r| r.text().contains(&secret)), "the agent reached outside a granted folder");
    assert!(results.iter().any(|t| t.contains("outside the folders")), "a path outside was not refused: {results:?}");

    // A ceiling only tightens: "allow" under "ask" still asks.
    let target = granted.join("new.txt");
    let model =
        FakeModel::start(steps(vec![contract_unchecked("write", "written"), tool("local_write_file", json!({ "path": target, "content": "made" })), complete("done")])).await;
    let app = Appliance::start(Spec::default().section("local", json!({ "ceiling": "ask" })), &model).await;
    let alice = app.api("alice");
    let _computer = connect_computer(&app, "alice", "laptop", "allow", &granted).await;
    let task = alice.submit("Write a file.").await;
    let item = alice.wait_item(&task, "approval").await;
    assert!(!item.is_null(), "a personal allow under a stricter ceiling did not ask");
    assert!(!target.exists(), "a personal allow under a stricter ceiling did not ask");
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    alice.wait_settled(&task).await;
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "made");
}

/// A command on a person's machine has no network unless it says so, and
/// saying so is a send Auto Review judges. With private data read on the
/// machine after untrusted input, it asks the person whatever the reviewer
/// says (spec rev 140, What still asks, case 3).
#[tokio::test(flavor = "multi_thread")]
async fn a_local_command_gets_the_network_only_as_a_reviewed_send() {
    let web = FakeWeb::start(&[("/p", "<html>page</html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    std::fs::write(path.join("private.txt"), canary("local-private")).unwrap();
    let _computer = connect_computer(&app, "alice", "laptop", "allow", &path).await;
    assert!(alice.put_policy("org", &org_policy(&permit_execute("local_exec"))).await.is_success());
    let fetch = format!("curl -s -m 3 {}/p", web.base);

    model.set_script(steps(vec![contract_unchecked("x", "x"), tool("local_exec", json!({ "command": fetch, "cwd": path })), complete("done")]));
    let task = alice.submit("Fetch without network.").await;
    alice.wait_settled(&task).await;
    assert!(web.hits().is_empty(), "a local command reached the network without asking for it");

    model.set_script(steps(vec![contract_unchecked("x", "x"), tool("local_exec", json!({ "command": fetch, "cwd": path, "network": true })), complete("done")]));
    let task = alice.submit("Install something.").await;
    alice.wait_settled(&task).await;
    assert!(!web.hits().is_empty(), "a command allowed the network could not use it");
    assert!(model.reviews().iter().any(|r| r.text().contains("local_exec")), "a command with the network went out without Auto Review");

    // With what it read on this machine in hand after reading a page, it does
    // not go out unasked, though the reviewer would let it.
    model.set_reviewer(script(|_| async { review("allow", "fine") }));
    let before = web.hits().len();
    model.set_script(steps(vec![
        contract_unchecked("x", "x"),
        tool("web_fetch", json!({ "url": web.url("/p") })),
        tool("local_read_file", json!({ "path": path.join("private.txt") })),
        tool("local_exec", json!({ "command": fetch, "cwd": path, "network": true })),
        complete("done"),
    ]));
    let task = alice.submit("Read that, then fetch.").await;
    let item = alice.wait_item(&task, "approval").await;
    assert_eq!(item["ask"]["allow_scope"], "once", "data read locally after untrusted input was not asked about as case 3: {item}");
    assert_eq!(web.hits().len(), before + 1, "data read locally left the machine without the person");
}

/// The agent keeps off a computer while its person is using it.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_yields_while_the_person_is_using_their_computer() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    let pause = state.path().join("paused");
    std::fs::write(&pause, b"").unwrap();
    let _computer = connect_computer_with(&app, "alice", "laptop", "allow", &path, &["--pause-file", pause.to_str().unwrap()]).await;
    let (_, listed) = alice.get("/v1/local/computers").await;
    let id = listed[0]["computer"]["id"].as_str().unwrap().to_string();
    let write = |n: &str| tool("local_write_file", json!({ "path": path.join(n), "content": "x" }));
    let wrote = |n: &str| {
        let file = path.join(n);
        async move { eventually("the job ran", PATIENCE, || async { file.exists().then_some(()) }).await }
    };

    model.set_script(steps(vec![contract_unchecked("w", "w"), write("first.txt"), write("second.txt"), complete("done")]));
    let task = alice.submit("Write them.").await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!path.join("first.txt").exists(), "the agent worked on a computer while its person was using it");
    assert_eq!(alice.get("/v1/local/computers").await.1[0]["in_use"], true, "the app cannot show why the agent is waiting");

    // Told to go ahead, it does, though they are still at the machine.
    let (code, v) = alice.post(&format!("/v1/local/computers/{id}/resume"), json!({})).await;
    assert_eq!(code, 200, "{v}");
    wrote("first.txt").await;

    // Going idle lets it carry on by itself.
    std::fs::remove_file(&pause).unwrap();
    wrote("second.txt").await;
    alice.wait_settled(&task).await;
    assert_eq!(app.api("bob").post(&format!("/v1/local/computers/{id}/resume"), json!({})).await.0, 404, "someone else told the agent to carry on");
}
