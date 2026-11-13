//! Invariant 6: autonomy comes from policy: the owner's for the
//! organisation, each person's own "Allow always" over their own work
//! within it. The agent can propose policy, never grant it, and has no tool
//! to propose or set a grant (spec rev 147: Allow always and connect-time
//! consent replace `propose_grant`).
//!
//! Prevents: an agent enabling its own auto-approve (CVE-2025-53773), and
//! autonomy that grows without anyone deciding it.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::time::Duration;

fn send(body: &str) -> Reply {
    tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": body }))
}

async fn mail_app(model: &FakeModel) -> (FakeMcp, Appliance) {
    let mail = mcp::mail().start().await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), model).await;
    (mail, app)
}

#[tokio::test(flavor = "multi_thread")]
async fn without_owner_policy_an_irreversible_action_waits_for_approval() {
    let model = FakeModel::start(steps(vec![contract_unchecked("say hi", "said"), send("hello"), fail("done")])).await;
    let (mail, app) = mail_app(&model).await;
    let alice = app.api("alice");
    let id = alice.submit("Email bob hello.").await;
    let item = alice.wait_item(&id, "approval").await;
    never("ran without approval", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("the approved send ran once", PATIENCE, || async { (mail.effects("send") == 1).then_some(()) }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_policy_grants_autonomy() {
    let model = FakeModel::start(steps(vec![contract_unchecked("say hi", "said"), send("hello"), fail("done")])).await;
    let (mail, app) = mail_app(&model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let id = alice.submit("Email bob hello.").await;
    alice.wait_settled(&id).await;
    assert_eq!(mail.effects("send"), 1);
    assert!(alice.open_items(&id, "approval").await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_proposes_and_only_the_owner_grants() {
    let model = FakeModel::start(steps(vec![
        contract_unchecked("say hi", "said"),
        tool("propose_policy", json!({ "layer": "org", "cedar": permit_execute("mail__send"), "rationale": "bob gets a hello every week" })),
        send("hello"),
        fail("done"),
    ]))
    .await;
    let (mail, app) = mail_app(&model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let id = alice.submit("Email bob hello.").await;
    alice.wait_item(&id, "approval").await;
    let proposal = alice.wait_item(&id, "policy_proposal").await;
    let pid = proposal["id"].as_str().unwrap().to_string();
    assert_eq!(mail.effects("send"), 0, "a proposal took effect by itself");

    for who in [&bob, &carol] {
        let st = who.respond(&pid, json!({ "decision": "accept" })).await;
        assert!(st == 401 || st == 403 || st == 404, "a person without the org policy role accepted a proposal: {st}");
    }
    assert!(alice.respond(&pid, json!({ "decision": "accept" })).await.is_success());

    // The new policy holds for new work.
    let next = alice.submit("Email bob hello again.").await;
    alice.wait_settled(&next).await;
    assert!(alice.open_items(&next, "approval").await.is_empty(), "the accepted policy did not apply");
}

/// The second send is identical to the first, so only the approval having
/// been used stops it.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_action_approval_covers_one_action() {
    let model = FakeModel::start(steps(vec![contract_unchecked("say hi twice", "said"), send("hello"), send("hello"), fail("done")])).await;
    let (mail, app) = mail_app(&model).await;
    let alice = app.api("alice");
    let id = alice.submit("Email bob twice.").await;
    let first = alice.wait_item(&id, "approval").await;
    let fid = first["id"].as_str().unwrap().to_string();
    assert!(alice.respond(&fid, json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("the first send ran", PATIENCE, || async { (mail.effects("send") == 1).then_some(()) }).await;

    let me = alice.clone();
    let (tid, fid2) = (id.clone(), fid.clone());
    eventually("a second approval is requested", PATIENCE, || {
        let (me, tid, fid) = (me.clone(), tid.clone(), fid2.clone());
        async move { me.open_items(&tid, "approval").await.into_iter().find(|i| i["id"] != fid.as_str()) }
    })
    .await;
    assert_eq!(mail.effects("send"), 1, "one approval covered two identical actions");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_approval_channel_needs_a_person() {
    let model = FakeModel::start(steps(vec![contract_unchecked("say hi", "said"), send("hello"), fail("done")])).await;
    let (mail, app) = mail_app(&model).await;
    let alice = app.api("alice");
    let id = alice.submit("Email bob hello.").await;
    let item = alice.wait_item(&id, "approval").await;
    let path = format!("{}/v1/inbox/{}", app.base, item["id"].as_str().unwrap());
    let http = reqwest::Client::new();
    let body = json!({ "decision": "approve", "scope": "once" });
    let anon = http.post(&path).json(&body).send().await.unwrap().status();
    let forged = http.post(&path).bearer_auth("tok-forged").json(&body).send().await.unwrap().status();
    assert_eq!(anon, 401);
    assert_eq!(forged, 401);
    never("an unauthenticated approval took effect", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;
    for p in &app.spec.people {
        assert!(model.requests().iter().all(|r| !r.text().contains(&p.token)), "a person's token reached the model");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn people_set_policy_only_within_their_role() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (bob, carol) = (app.api("bob"), app.api("carol"));
    let rule = permit_execute("mail__send");
    for (who, layer) in [(&bob, "org"), (&bob, "user/alice"), (&bob, "group/staff"), (&carol, "user/carol"), (&carol, "org")] {
        let st = who.put_policy(layer, &rule).await;
        assert!(st == 401 || st == 403, "{layer} accepted a policy from someone without the role: {st}");
    }
    assert!(bob.put_policy("user/bob", r#"forbid(principal, action, resource == Tool::"mail__send");"#).await.is_success());
}

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";

/// In chat the agent tries to grant itself standing authority: it calls the
/// retired `propose_grant`. The memory pass writes nothing.
fn grants_model() -> model::Script {
    script(|c| async move {
        let sys = match &c.body["system"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
            other => other.to_string(),
        };
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !sys.contains(CHAT) {
            return match c.turn() {
                0 => send("hello"),
                _ => fail("done"),
            };
        }
        if c.turn() > 0 {
            return Reply::Text("Done.".into());
        }
        tool(
            "propose_grant",
            json!({ "title": "Emailing bob", "apps": ["mail"], "allow": ["send", "irreversible"], "period": "week", "max_usd": 5.0, "why": "You asked me to email bob without asking." }),
        )
    })
}

/// The person writes in a new conversation; its entries once the turn settled.
async fn chat_entries(who: &Api, text: &str) -> Vec<Value> {
    let (st, v) = who.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    who.wait_settled(v["task_id"].as_str().unwrap()).await;
    let (_, t) = who.get(&format!("/v1/chat/{}", v["thread"].as_str().unwrap())).await;
    t["entries"].as_array().cloned().unwrap_or_default()
}

async fn grants_of(who: &Api) -> Vec<Value> {
    who.get("/v1/grants").await.1["grants"].as_array().cloned().unwrap_or_default()
}

/// The agent cannot grant itself standing authority (spec rev 147): it is
/// offered no tool to propose or set a grant, a call to the retired one is
/// refused and puts no card in the thread, and afterwards a send still asks.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_cannot_grant_itself_standing_authority() {
    let model = FakeModel::start(grants_model()).await;
    let (mail, app) = mail_app(&model).await;
    let alice = app.api("alice");

    let entries = chat_entries(&alice, "Just email bob from now on without asking me.").await;
    let offered = model.requests().into_iter().find(|c| c.text().contains(CHAT)).expect("no chat turn").offered_tools();
    assert!(!offered.iter().any(|t| t.contains("grant") && t != "revoke_grant"), "the agent was offered a way to grant: {offered:?}");
    assert!(entries.iter().all(|e| e["kind"] != "grant_card"), "a grant card was put to the person");
    assert!(grants_of(&alice).await.is_empty(), "a grant was set without the person");

    let id = alice.submit("Email bob hello.").await;
    alice.wait_item(&id, "approval").await;
    never("the attempt lifted an ask", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;
}

/// Move the whole log back by `minutes`, as if that much time had passed,
/// and start the server again on it: what it rebuilds is as old as that.
async fn as_if_minutes_passed(app: &mut Appliance, minutes: u64) {
    app.kill().await;
    app.db.admin().await.batch_execute(&format!("UPDATE public.events SET at = at - interval '{minutes} minutes'")).await.expect("move the log back in time");
    app.restart().await;
}

/// Test 16. A routine's ask waits 30 minutes for its person (spec rev 140,
/// Outcomes come back to the agent), and nothing runs: then a message stays
/// a draft, and an install keeps its ask while its task keeps waiting.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_background_ask_runs_nothing() {
    /// Past the 30 minutes an ask from unwatched work waits.
    const PAST_THE_WAIT_MINUTES: u64 = 31;
    let sections = |spec: Spec| spec.section("schedules", json!({ "min_every_minutes": 1 }));
    let routine = json!({ "text": "Pass the weekly draft on.", "every_minutes": 1, "max_runs": 1 });
    let empty = json!({ "type": "object", "properties": { "draft_id": { "type": "string" } } });

    // A message.
    let model = FakeModel::start(steps(vec![
        contract_unchecked("send it", "sent"),
        tool("drafts__get_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d1" })),
        fail("not sent"),
    ]))
    .await;
    let drafts = FakeMcp::builder()
        .world("drafts", json!({ "d1": { "to": "someone@outside.example", "subject": "weekly", "body": "Confidential" } }))
        .tool("get_draft", "Read a draft.", empty.clone(), |w, a, _| {
            mcp::ok(json!({ "draft_id": a["draft_id"], "message": w["drafts"][a["draft_id"].as_str().unwrap_or_default()].clone() }))
        })
        .tool("send_draft", "Send a draft.", empty.clone(), |_, _, _| mcp::ok(json!({ "status": "sent" })))
        .start()
        .await;
    let spec = sections(Spec::default()).app("drafts", &drafts, "public", "private+untrusted", &[("get_draft", "read"), ("send_draft", "irreversible")]).destinations(
        "drafts",
        "send_draft",
        "get_draft",
        "draft_id",
        &["/message/to"],
        &["/message/body"],
    );
    let mut app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", "permit(principal, action, resource);").await.is_success());
    let (code, body) = alice.post("/v1/schedules", routine.clone()).await;
    assert!(code.is_success(), "{body}");
    let ask = eventually("the routine's ask", PATIENCE, || async { alice.inbox().await.into_iter().find(|i| i["kind"] == "approval" && i["status"] == "open") }).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(alice.inbox().await.iter().all(|i| i["kind"] != "draft"), "an ask became a draft before its wait was over");
    as_if_minutes_passed(&mut app, PAST_THE_WAIT_MINUTES).await;
    let draft = eventually("the expired ask's draft", PATIENCE, || async { alice.inbox().await.into_iter().find(|i| i["kind"] == "draft") }).await;
    assert_eq!(draft["details"]["from_ask"], ask["id"], "the draft is not from the ask");
    assert!(alice.inbox().await.iter().all(|i| !(i["kind"] == "approval" && i["status"] == "open")), "the expired ask is still open");
    assert!(drafts.calls_to("send_draft").is_empty(), "an expired ask ran");

    // An install waits with its ask.
    let model = FakeModel::start(steps(vec![contract_unchecked("install it", "installed"), tool("apps__install_app", json!({})), fail("not installed")])).await;
    model.set_reviewer(script(|_| async { review("ask", "a person should decide") }));
    let apps =
        FakeMcp::builder().tool("install_app", "Install an app.", json!({ "type": "object", "properties": {} }), |_, _, _| mcp::ok(json!({ "status": "installed" }))).start().await;
    let spec = sections(Spec::default()).app("apps", &apps, "internal", "private", &[("install_app", "irreversible")]);
    let mut app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", "permit(principal, action, resource);").await.is_success());
    let (code, body) = alice.post("/v1/schedules", routine).await;
    assert!(code.is_success(), "{body}");
    let ask = eventually("the routine's install ask", PATIENCE, || async { alice.inbox().await.into_iter().find(|i| i["kind"] == "approval" && i["status"] == "open") }).await;
    as_if_minutes_passed(&mut app, PAST_THE_WAIT_MINUTES).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let inbox = alice.inbox().await;
    assert!(inbox.iter().any(|i| i["id"] == ask["id"] && i["status"] == "open"), "the install's ask did not wait");
    assert!(inbox.iter().all(|i| i["kind"] != "draft"), "an install became a draft");
    assert!(apps.calls_to("install_app").is_empty(), "an expired ask ran");
}

/// Test 17. Disclosing a credential and reaching another person's account
/// or computer are refused whatever is permitted, and no one is asked.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_permitted_unlocks_a_hard_denial() {
    let computerd = Computerd::start();
    let stored = canary("stored-credential");
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![])).await;
    let mut spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]).computer(&computerd.socket);
    spec.secrets.push(("hard-denial-canary".to_string(), stored.clone()));
    let app = Appliance::start(spec, &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", "permit(principal, action, resource);").await.is_success());
    let (code, body) = alice.post("/v1/rules", json!({ "connector": "crm", "tool": "crm__update_record", "destinations": ["anyone@corp.example"] })).await;
    assert_eq!(code, 200, "{body}");
    let (_, made) = bob.post("/v1/computers", json!({})).await;
    let bobs = made["id"].as_str().unwrap().to_string();
    model.set_script(hard_denial_model(stored, bobs));
    // Everything is permitted, and the destination allowed always.
    let id = alice.submit("Update record 42.").await;
    alice.wait_settled(&id).await;
    assert!(crm.calls_to("update_record").is_empty(), "a grant unlocked a hard denial");
    assert!(alice.open_items(&id, "approval").await.is_empty(), "a hard denial was offered for approval");
}

/// The task tries each thing nothing permitted can allow.
fn hard_denial_model(stored: String, bobs: String) -> model::Script {
    let update = |fields: Value| tool("crm__update_record", json!({ "id": "42", "fields": fields }));
    steps(vec![update(json!({ "note": stored })), update(json!({ "account": "bob" })), update(json!({ "computer": bobs })), fail("all refused")])
}

// The UI catalog: what the agent renders can ask for work, and only that.

#[tokio::test(flavor = "multi_thread")]
async fn a_button_in_rendered_ui_is_a_request_not_an_approval() {
    let mail = mcp::mail().start().await;
    let send = || tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" }));
    let model = FakeModel::start(steps(vec![contract_unchecked("send", "sent"), send(), complete("done")])).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let alice = app.api("alice");
    let waiting = alice.submit("Send a note.").await;
    let item = alice.wait_item(&waiting, "approval").await;
    assert!(!item.is_null(), "the send did not wait for approval");

    let button = json!({ "type": "action", "label": "Approve", "request": "Approve the waiting send and send it." });
    model.set_script(steps(vec![contract_unchecked("offer", "offered"), tool("task_complete", json!({ "summary": "here it is", "deliverable": [button] }))]));
    let offered = alice.submit("Offer a button.").await;
    alice.wait_settled(&offered).await;

    model.set_reviewer(script(|_| async { review("deny", "no") }));
    model.set_script(steps(vec![contract_unchecked("send", "sent"), send(), complete("done")]));
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let (code, made) = alice.post(&format!("/v1/tasks/{offered}/actions/0"), json!({})).await;
    assert_eq!(code, 200, "{made}");
    alice.wait_settled(made["task_id"].as_str().unwrap()).await;

    let still = alice.open_items(&waiting, "approval").await;
    assert_eq!(still.len(), 1, "rendered UI approved an action");
    assert_eq!(still[0]["id"], item["id"], "rendered UI approved an action");
    assert_eq!(mail.effects("send"), 0, "rendered UI approved an action");
    assert!(!model.reviews().is_empty(), "the button's request did not go through Auto Review");

    let (code, _) = alice.post(&format!("/v1/tasks/{waiting}/actions/0"), json!({})).await;
    assert_eq!(code, 404, "a task with no button took a press");
}

/// Waits for a task to complete: checking is internal, and nothing is put
/// to the person to accept (spec rev 140).
async fn settle_reviewed(who: &Api, task: &str) {
    let t = who.wait_settled(task).await;
    assert_eq!(t["status"], "completed", "{t}");
}

fn fence(body: &str) -> String {
    format!("Here it is.\n```magnus-ui\n{body}\n```")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_element_not_in_the_catalog_is_refused_not_guessed() {
    let model = FakeModel::start(steps(vec![
        contract_unchecked("show", "shown"),
        Reply::Text(fence("marquee Hello")),
        tool("task_complete", json!({ "summary": fence("marquee Hello") })),
        complete("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Show something.").await;
    alice.wait_settled(&task).await;
    let told = model.requests().iter().any(|r| r.text().contains("cannot be drawn") && r.text().contains("marquee"));
    assert!(told, "an unknown element was not sent back");

    let (code, catalog) = alice.get("/v1/ui/catalog").await;
    assert_eq!(code, 200, "{catalog}");
    let names: Vec<&str> = catalog["elements"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"form") && !names.contains(&"marquee"), "{names:?}");
    let csp = catalog["html_csp"].as_str().unwrap();
    assert!(csp.starts_with("default-src 'none'") && !csp.contains("connect-src") && !csp.contains("http"), "agent HTML could reach the network: {csp}");
    let (code, _) = alice.put("/v1/ui/catalog", json!({ "version": 999, "elements": ["text"] })).await;
    assert_eq!(code, 409, "a client on another catalog was accepted");

    // What the client says it can draw is all that may be written for it.
    assert!(alice.put("/v1/ui/catalog", json!({ "version": catalog["version"], "elements": ["text"] })).await.0.is_success());
    let before = model.requests().len();
    model.set_script(steps(vec![contract_unchecked("show", "shown"), Reply::Text(fence("stat label=Revenue value=1")), complete("done")]));
    let task = alice.submit("Show a number.").await;
    alice.wait_settled(&task).await;
    assert!(model.requests()[before..].iter().any(|r| r.text().contains("cannot be drawn")), "an element this client cannot draw was accepted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_form_is_a_request_with_only_the_answers_it_asked_for() {
    let mail = mcp::mail().start().await;
    let send = || tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" }));
    let model = FakeModel::start(steps(vec![contract_unchecked("send", "sent"), send(), complete("done")])).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let alice = app.api("alice");
    let waiting = alice.submit("Send a note.").await;
    let item = alice.wait_item(&waiting, "approval").await;

    let form = "form title=Details submit=Send request=\"Approve the waiting send\"\n  field name=email label=Email required";
    let answer = serde_json::to_string(&fence(form)).unwrap();
    let summary: serde_json::Value = serde_json::from_str(&answer).unwrap();
    model.set_script(steps(vec![
        contract_unchecked("offer", "offered"),
        tool("task_complete", json!({ "summary": summary, "deliverable": [{ "type": "text", "text": "offered" }] })),
    ]));
    let offered = alice.submit("Offer a form.").await;
    settle_reviewed(&alice, &offered).await;

    let before = model.requests().len();
    model.set_script(steps(vec![contract_unchecked("read", "read"), complete("done")]));
    let extra = canary("not-asked-for");
    let (code, made) = alice.post(&format!("/v1/tasks/{offered}/actions/0"), json!({ "fence": 0, "values": { "email": "ada@corp.example", "evil": extra } })).await;
    assert_eq!(code, 200, "{made}");
    let started = made["task_id"].as_str().unwrap().to_string();
    alice.wait_settled(&started).await;
    let first = &model.requests()[before];
    assert!(first.text().contains("ada@corp.example"), "the form's answer did not arrive");
    assert!(!first.text().contains(&extra), "a field the form never asked for arrived");
    assert_eq!(alice.task(&started).await["taint"]["untrusted"], json!(true), "a form's answers were trusted");
    assert_eq!(alice.open_items(&waiting, "approval").await[0]["id"], item["id"], "rendered UI approved an action");
    assert_eq!(mail.effects("send"), 0, "rendered UI approved an action");
    let (code, _) = alice.post(&format!("/v1/tasks/{offered}/actions/7"), json!({ "fence": 0, "values": {} })).await;
    assert_eq!(code, 404, "an element that does not exist took a press");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_is_a_search_and_replace_that_must_still_draw() {
    let answer = "Here.\n```magnus-ui id=rev\nstat label=Revenue value=1\n```".to_string();
    let model = FakeModel::start(steps(vec![Reply::Text(answer)])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (st, first) = alice.post("/v1/chat", json!({ "text": "Show revenue." })).await;
    assert_eq!(st.as_u16(), 201, "{first}");
    alice.wait_settled(first["task_id"].as_str().unwrap()).await;
    let thread = first["thread"].as_str().unwrap().to_string();
    let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
    let shown = t["entries"].as_array().cloned().unwrap_or_default();
    assert!(shown.iter().any(|e| e["text"].as_str().unwrap_or_default().contains("value=1")), "{shown:?}");

    model.set_script(steps(vec![
        tool("ui_edit", json!({ "id": "rev", "find": "value=1", "replace": "value=2" })),
        tool("ui_edit", json!({ "id": "rev", "find": "not there", "replace": "x" })),
        tool("ui_edit", json!({ "id": "rev", "find": "stat", "replace": "marquee" })),
        Reply::Text("Changed.".into()),
    ]));
    chat_entries(&alice, "Make it two.").await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("Edited rev")), "{results:?}");
    assert_eq!(results.iter().filter(|t| t.contains("Nothing was changed")).count(), 2, "{results:?}");
    let (_, v) = alice.get(&format!("/v1/chat/{thread}")).await;
    let seen = v.to_string();
    assert!(seen.contains("value=2") && !seen.contains("value=1"), "the edit did not take: {seen}");
    assert!(!seen.contains("marquee"), "an edit that cannot draw was kept: {seen}");
}

async fn bytes(who: &Api, path: &str, token: &str) -> (reqwest::StatusCode, Vec<u8>) {
    let r = reqwest::Client::new().get(format!("{}{path}", who.base)).bearer_auth(token).send().await.unwrap();
    (r.status(), r.bytes().await.unwrap().to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deck_exports_to_its_owner_alone() {
    let answer =
        "Here.\n```magnus-ui id=pitch\ndeck title=Pitch\n  slide layout=title title=Hello subtitle=There\n  slide layout=bullets title=Wins items=One,Two\n```".to_string();
    let model = FakeModel::start(steps(vec![Reply::Text(answer)])).await;
    let spec = Spec::default();
    let (alice_token, bob_token) = (spec.token("alice"), spec.token("bob"));
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    chat_entries(&alice, "Make a deck.").await;
    let (code, pptx) = bytes(&alice, "/v1/ui/docs/pitch/export/pptx", &alice_token).await;
    assert_eq!(code, 200);
    assert_eq!(&pptx[..2], b"PK", "not a zip");
    let (code, pdf) = bytes(&alice, "/v1/ui/docs/pitch/export/pdf", &alice_token).await;
    assert_eq!(code, 200);
    assert_eq!(&pdf[..5], b"%PDF-");
    assert_eq!(bytes(&alice, "/v1/ui/docs/pitch/export/pptx", &bob_token).await.0, 404, "another person read the deck");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_image_is_made_behind_a_placeholder_and_goes_with_its_run() {
    let picture = "aGVsbG8tcGljdHVyZQ==";
    let web = FakeWeb::start(&[("/images/generations", &format!("{{\"data\":[{{\"b64_json\":\"{picture}\"}}]}}"))]).await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("draw", "drawn"),
        tool("image_generate", json!({ "prompt": "a red barn" })),
        tool("image_generate", json!({ "prompt": "a blue barn", "quality": "premium" })),
        complete("done"),
    ]))
    .await;
    let mut spec = Spec::default().section("images", json!({ "url": web.url("/images"), "key": "images_key", "fast_model": "quick", "premium_model": "best" }));
    let key = canary("image-key");
    spec.secrets.push(("images_key".into(), key.clone()));
    let (alice_token, bob_token) = (spec.token("alice"), spec.token("bob"));
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Draw a barn.").await;
    settle_reviewed(&alice, &task).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    let told = results.iter().find(|t| t.contains("placeholder=")).unwrap_or_else(|| panic!("{results:?}"));
    let id = told.split("placeholder=").nth(1).unwrap().split(['.', ' ']).take(2).collect::<Vec<_>>().join(".");

    let path = format!("/v1/ui/images/{id}");
    let got = eventually("the image", Duration::from_secs(10), || {
        let (who, token, path) = (&alice, &alice_token, &path);
        async move {
            let (code, b) = bytes(who, path, token).await;
            (code == 200).then_some(b)
        }
    })
    .await;
    assert_eq!(got, b"hello-picture");
    assert_eq!(bytes(&alice, &path, &bob_token).await.0, 404, "another person read the image");

    let made = web.seen();
    assert!(made.iter().all(|r| r.authorization.as_deref() == Some(format!("Bearer {key}").as_str())), "the key did not come from the vault");
    assert!(made.iter().any(|r| r.body.contains("quick") && r.body.contains("red barn")), "the fast model is not the default");
    assert!(made.iter().any(|r| r.body.contains("best") && r.body.contains("blue barn")), "premium was not used when asked");

    assert_eq!(alice.post("/v1/trash", json!({ "item": task })).await.0, 204);
    assert_eq!(alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{task}"), None).await.0, 204);
    assert_ne!(bytes(&alice, &path, &alice_token).await.0, 200, "the image outlived its run");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connectors_view_is_kept_for_its_person_and_can_only_ask() {
    let crm = mcp::crm().view("get_record", "ui://crm/record", "<p>the record view</p>").start().await;
    let model = FakeModel::start(steps(vec![contract_unchecked("look", "looked"), tool("crm__get_record", json!({ "id": "42" })), complete("done")])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]);
    let bob_token = spec.token("bob");
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Look at record 42.").await;
    alice.wait_settled(&task).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    let told = results.iter().find(|t| t.contains("app view=")).unwrap_or_else(|| panic!("the tool's view was not offered: {results:?}"));
    let id = told.split("app view=").nth(1).unwrap().split('`').next().unwrap().to_string();

    let (code, view) = alice.get(&format!("/v1/ui/apps/{id}")).await;
    assert_eq!(code, 200, "{view}");
    assert_eq!(view["html"], "<p>the record view</p>");
    assert_eq!(view["input"]["id"], "42");
    assert_eq!(bytes(&alice, &format!("/v1/ui/apps/{id}"), &bob_token).await.0, 404, "another person read the view");

    let before = crm.effects("update_record");
    let (code, made) = alice.post(&format!("/v1/ui/apps/{id}/request"), json!({ "tool": "update_record", "arguments": { "id": "42" } })).await;
    assert_eq!(code, 200, "{made}");
    model.set_script(steps(vec![contract_unchecked("read", "read"), complete("done")]));
    let started = made["task_id"].as_str().unwrap();
    alice.wait_settled(started).await;
    assert_eq!(alice.task(started).await["taint"]["untrusted"], json!(true), "what a view asked for was trusted");
    assert_eq!(crm.effects("update_record"), before, "a view ran a tool by itself");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_screenshots_its_html_with_no_network() {
    let computerd = Computerd::start();
    let web = FakeWeb::start(&[("/asked", "x")]).await;
    let html = format!("<h1>Made here</h1><img src=\"{0}\"><script>fetch(\"{0}\")</script>", web.url("/asked"));
    let answer = format!("Here.\n```magnus-html id=page\n{html}\n```");
    let model = FakeModel::start(steps(vec![Reply::Text(answer)])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await.0, 200);
    chat_entries(&alice, "Make a page.").await;
    model.set_script(steps(vec![tool("html_screenshot", json!({ "id": "page" })), Reply::Text("Looks right.".into())]));
    chat_entries(&alice, "Check it.").await;
    let last = model
        .requests()
        .into_iter()
        .find(|r| r.last_results().iter().any(|x| x.text.contains("Screenshot of page")))
        .unwrap_or_else(|| panic!("no screenshot: {:?}", model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect::<Vec<_>>()));
    assert!(serde_json::to_string(&last.messages()).unwrap().contains("\"image\""), "the model was not shown the picture");
    assert!(web.hits().is_empty(), "the page reached the network: {:?}", web.hits());
}
