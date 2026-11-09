//! Invariant 6: autonomy comes from policy: the owner's for the
//! organisation, each person's own grants over their own work within it.
//! The agent can propose policy and grants, never grant them.
//!
//! Prevents: an agent enabling its own auto-approve (CVE-2025-53773), and
//! autonomy that grows without anyone deciding it.

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

#[tokio::test(flavor = "multi_thread")]
async fn a_one_action_approval_covers_one_action() {
    let model = FakeModel::start(steps(vec![contract_unchecked("say hi twice", "said"), send("first"), send("second"), fail("done")])).await;
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
    assert_eq!(mail.effects("send"), 1, "one approval covered two actions");
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

/// In chat, `grant` asks for a grant over sending mail (sends and what
/// cannot be undone lifted), and `run <id>` starts a task under grant
/// `id`; a task emails bob hello. The memory pass writes nothing.
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
                0 => contract_unchecked("say hi", "said"),
                1 => send("hello"),
                _ => fail("done"),
            };
        }
        if c.turn() > 0 {
            return Reply::Text("Done.".into());
        }
        let said = c.messages().first().and_then(|m| m["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).map(String::from)).unwrap_or_default();
        match said.strip_prefix("run ") {
            Some(g) => tool("create_task", json!({ "title": "Email bob", "text": "Email bob hello.", "grant": g })),
            None => tool(
                "propose_grant",
                json!({ "title": "Emailing bob", "apps": ["mail"], "allow": ["send", "irreversible"], "period": "week", "max_usd": 5.0, "why": "You asked me to email bob without asking." }),
            ),
        }
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

/// The task a `run` message started, if one did.
async fn run_under(who: &Api, grant: &str) -> Option<String> {
    let entries = chat_entries(who, &format!("run {grant}")).await;
    entries.iter().find(|e| e["kind"] == "tasks").and_then(|e| e["tasks"][0]["task_id"].as_str().map(String::from))
}

async fn grants_of(who: &Api) -> Vec<Value> {
    who.get("/v1/grants").await.1["grants"].as_array().cloned().unwrap_or_default()
}

/// A person's grant over their own work (owner's ruling, 2026-11-02; #235):
/// the agent's proposal alone grants nothing (a send still asks), only that
/// person's accept sets one, no one else can accept it, and it never covers
/// what its person could not approve themselves.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_proposes_a_grant_and_only_its_person_grants_what_they_could_approve() {
    let model = FakeModel::start(grants_model()).await;
    let (mail, app) = mail_app(&model).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));

    // Carol (no approver role) asks; the agent's card alone grants nothing.
    let card = chat_entries(&carol, "grant").await.into_iter().find(|e| e["kind"] == "grant_card").expect("no grant card");
    let card = card["card"].as_str().unwrap().to_string();
    assert!(grants_of(&carol).await.is_empty(), "a proposal took effect by itself");
    assert_eq!(run_under(&carol, &card).await, None, "a task started under a card nobody accepted");
    let id = carol.submit("Email bob hello.").await;
    carol.wait_item(&id, "approval").await;

    // Only Carol accepts it.
    for who in [&alice, &bob] {
        let (st, _) = who.post(&format!("/v1/chat/cards/{card}"), json!({ "decision": "accept" })).await;
        assert_eq!(st.as_u16(), 404, "someone else accepted carol's grant");
    }
    assert!(grants_of(&carol).await.is_empty());
    let (st, v) = carol.post(&format!("/v1/chat/cards/{card}"), json!({ "decision": "accept" })).await;
    assert!(st.is_success(), "{v}");
    let grant = v["grant"].as_str().unwrap().to_string();

    // Her grant lifts sends and what cannot be undone, but she cannot
    // approve an irreversible send herself: under it, the send still asks.
    let task = run_under(&carol, &grant).await.expect("no task under carol's grant");
    carol.wait_item(&task, "approval").await;
    never("a grant covered what its person could not approve", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;

    // The owner's own grant, accepted by her, covers it for her.
    let card = chat_entries(&alice, "grant").await.into_iter().find(|e| e["kind"] == "grant_card").expect("no grant card");
    let (st, v) = alice.post(&format!("/v1/chat/cards/{}", card["card"].as_str().unwrap()), json!({ "decision": "accept" })).await;
    assert!(st.is_success(), "{v}");
    let task = run_under(&alice, v["grant"].as_str().unwrap()).await.expect("no task under alice's grant");
    alice.wait_settled(&task).await;
    assert_eq!(mail.effects("send"), 1, "the owner's grant did not cover her send");
    assert!(alice.open_items(&task, "approval").await.is_empty());
}
