//! Standing authority (behavior spec, Standing authority; behavior.md step
//! 5; #235). A grant comes only from the person: "Handle things like this"
//! on a task proposal, or accepting a grant card Magnus put to them when
//! they asked; approving a proposal never creates one. A grant is a Cedar
//! policy the kernel writes from what they chose: the apps it covers, a
//! budget of model cost per rolling period, and what still asks (spending
//! money always; sending on their behalf and what cannot be undone unless
//! lifted). In a task started under it, what the organisation's policy
//! asks about goes ahead within it; the kernel's own asks (Rule of Two)
//! still ask. Grants are listed with the actions taken under each, and are
//! edited and revoked by their person alone, in Settings or by telling
//! Magnus.

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const GRANTS: &str = "# Work this person lets you do on your own";
const LEDGER: &str = "# Your suggestions to this person and their answers";

/// The owner's policy here: Magnus reads and works in its sandbox on its
/// own, and asks before any change in the CRM or the team chat (and before
/// anything else it does not permit).
const STRICT: &str = r#"
permit(principal, action == Action::"execute", resource)
  when { resource.risk == "read" || resource.risk == "sandbox" || (resource.risk == "reversible_write" && !(resource in App::"crm") && !(resource in App::"team")) };
permit(principal, action == Action::"request_approval", resource);
"#;

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn system(c: &Conversation) -> String {
    match &c.body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

/// The id of this turn's call of `tool`.
fn call_of(c: &Conversation, tool: &str) -> Value {
    let blocks = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default());
    blocks.into_iter().find(|b| b["type"] == "tool_use" && b["name"] == tool).map(|b| b["id"].clone()).unwrap_or_else(|| panic!("no {tool} call"))
}

/// The word after `key=` in `text`.
fn arg(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("{key}="))? + key.len() + 1;
    let w: String = text[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (!w.is_empty()).then_some(w)
}

/// What a task's plan does, by its name in what it was asked.
fn plan(name: &str) -> Vec<Reply> {
    let post = || tool("team__post", json!({ "channel": "general", "text": "Status: done." }));
    match name {
        "update" => vec![tool("crm__update_record", json!({ "id": "42", "fields": { "status": "done" } }))],
        "post" => vec![post()],
        "delete" => vec![tool("crm__delete_all", json!({}))],
        "report" => vec![tool("finance__get_report", json!({ "quarter": "Q3" }))],
        "inbox-post" => vec![tool("mail__list_inbox", json!({})), post()],
        _ => vec![],
    }
}

/// The person's messages drive each chat turn:
/// - `grant:{...}` puts a grant card with those fields;
/// - `run plan=<p> grant=<g>` starts a task doing plan p under grant g;
/// - `propose plan=<p>` reads record 42 and proposes a task doing plan p;
/// - `revoke grant=<g>` revokes grant g.
///
/// A task does its plan after its contract; the memory pass writes nothing.
fn model() -> model::Script {
    script(|c| async move {
        let sys = system(&c);
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !sys.contains(CHAT) {
            let steps = plan(&arg(&c.text(), "plan").unwrap_or_default());
            return match c.turn() {
                0 => contract_unchecked("do the plan", "it is done"),
                n if n <= steps.len() => steps[n - 1].clone(),
                _ => fail("done"),
            };
        }
        let text = said(&c);
        let first = |r: Reply| if c.turn() == 0 { r } else { Reply::Text("The card awaits you.".into()) };
        if let Some(fields) = text.strip_prefix("grant:") {
            return first(tool("propose_grant", serde_json::from_str(fields).unwrap()));
        }
        if text.starts_with("run ") {
            let p = arg(&text, "plan").unwrap();
            let mut input = json!({ "title": format!("Do {p}"), "text": format!("Do the work: plan={p}.") });
            if let Some(g) = arg(&text, "grant") {
                input["grant"] = json!(g);
            }
            return first(tool("create_task", input));
        }
        if text.starts_with("revoke ") {
            return first(tool("revoke_grant", json!({ "grant": arg(&text, "grant").unwrap() })));
        }
        if text.starts_with("propose ") {
            let p = arg(&text, "plan").unwrap();
            return match c.turn() {
                0 => tool("crm__get_record", json!({ "id": "42" })),
                1 => tool(
                    "propose_task",
                    json!({
                        "title": format!("Close record 42 ({p})"), "text": format!("Mark record 42 done: plan={p}."),
                        "noticed": { "what": "Record 42 is still open", "where": "the CRM" },
                        "outcome": "Record 42 marked done.", "why_now": "It was finished yesterday.",
                        "needs": { "permissions": ["Changes record 42 in the CRM"], "estimated_cost_usd": 0.5 },
                        "evidence": [{ "call": call_of(&c, "crm__get_record"), "shows": "Record 42 is open." }],
                    }),
                ),
                _ => Reply::Text("The card awaits you.".into()),
            };
        }
        Reply::Text("Noted.".into())
    })
}

/// The apps: a CRM (private), team chat (internal), finance reports (each
/// read classed as moving money) and a mailbox whose mail is private and
/// written by others.
struct World {
    crm: FakeMcp,
    team: FakeMcp,
    _finance: FakeMcp,
    _mail: FakeMcp,
    app: Appliance,
    m: FakeModel,
}

async fn world() -> World {
    world_with(false).await
}

/// The same apps, with the verifier on or off.
async fn world_with(verifier: bool) -> World {
    let m = FakeModel::start(model()).await;
    let (crm, team, finance, mail) = (mcp::crm().start().await, mcp::chat().start().await, mcp::finance().start().await, mcp::mail().start().await);
    let spec = Spec::default()
        .app("crm", &crm, "private", "principal", &[("get_record", "read"), ("update_record", "reversible_write"), ("delete_all", "irreversible")])
        .app("team", &team, "internal", "principal", &[("post", "reversible_write"), ("list_posts", "read")])
        .app("finance", &finance, "private", "principal", &[("get_report", "financial")])
        .app("mail", &mail, "private", "private+untrusted", &[("list_inbox", "read")]);
    let mut spec = spec;
    spec.verifier = verifier;
    let app = Appliance::start(spec, &m).await;
    assert!(app.api("alice").put_policy("org", STRICT).await.is_success());
    World { crm, team, _finance: finance, _mail: mail, app, m }
}

/// The person writes in a new conversation; it returns once the turn settled.
async fn chat(api: &Api, text: &str) -> String {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    api.wait_settled(v["task_id"].as_str().unwrap()).await;
    v["thread"].as_str().unwrap().to_string()
}

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/chat/{thread}")).await;
    assert!(st.is_success(), "{v}");
    v["entries"].as_array().cloned().unwrap_or_default()
}

async fn wait_entry(api: &Api, thread: &str, kind: &str) -> Value {
    for _ in 0..600 {
        if let Some(e) = entries(api, thread).await.into_iter().find(|e| e["kind"] == kind) {
            return e;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("no {kind} in the thread: {:?}", entries(api, thread).await);
}

async fn decide(api: &Api, card: &str, body: Value) -> (u16, Value) {
    let (st, v) = api.post(&format!("/v1/chat/cards/{card}"), body).await;
    (st.as_u16(), v)
}

/// The person asks for a grant with these fields and accepts its card as put.
async fn grant(api: &Api, fields: Value) -> String {
    let thread = chat(api, &format!("grant:{fields}")).await;
    let card = wait_entry(api, &thread, "grant_card").await;
    let (st, v) = decide(api, card["card"].as_str().unwrap(), json!({ "decision": "accept" })).await;
    assert_eq!(st, 200, "{v}");
    v["grant"].as_str().expect("no grant set").to_string()
}

fn fields(title: &str, apps: &[&str], max_usd: f64) -> Value {
    json!({ "title": title, "apps": apps, "period": "week", "max_usd": max_usd, "why": "You asked me to handle these on my own." })
}

/// The task a `run` message started, or none when it was refused.
async fn run(api: &Api, plan: &str, grant: Option<&str>) -> Option<String> {
    let g = grant.map(|g| format!(" grant={g}")).unwrap_or_default();
    let thread = chat(api, &format!("run plan={plan}{g}")).await;
    entries(api, &thread).await.into_iter().find(|e| e["kind"] == "tasks").and_then(|e| e["tasks"][0]["task_id"].as_str().map(String::from))
}

async fn grants(api: &Api) -> Vec<Value> {
    let (st, v) = api.get("/v1/grants").await;
    assert!(st.is_success(), "{v}");
    v["grants"].as_array().cloned().unwrap_or_default()
}

async fn grant_of(api: &Api, id: &str) -> Value {
    grants(api).await.into_iter().find(|g| g["id"] == id).unwrap_or_else(|| panic!("no grant {id}"))
}

/// The task's action went ahead without asking: `tool` of `app` ran once more.
async fn went_ahead(api: &Api, app: &FakeMcp, tool: &str, before: usize, task: &str) {
    eventually(&format!("{tool} went ahead in {task}"), PATIENCE, || async { (app.effects(tool) == before + 1).then_some(()) }).await;
    api.wait_settled(task).await;
    assert!(api.open_items(task, "approval").await.is_empty(), "{tool} asked as well");
}

/// The task's action asked, and did not happen.
async fn asked(api: &Api, app: Option<&FakeMcp>, tool: &str, task: &str) -> Value {
    let before = app.map(|a| a.effects(tool));
    let item = api.wait_item(task, "approval").await;
    if let (Some(a), Some(b)) = (app, before) {
        never(&format!("{tool} ran without approval"), Duration::from_secs(2), || async { a.effects(tool) > b }).await;
    }
    item
}

/// The turn after a tool call in the conversation that `text` started.
fn after(m: &FakeModel, text: &str) -> Conversation {
    m.requests().into_iter().rev().find(|c| c.turn() == 1 && said(c) == text && system(c).contains(CHAT)).unwrap_or_else(|| panic!("no second turn for {text:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn handling_things_like_this_sets_a_grant_whose_tasks_go_ahead_and_approving_a_proposal_sets_none() {
    let w = world().await;
    let alice = w.app.api("alice");

    // "Handle things like this" on a proposal: a grant from its defaults,
    // and its task starts under it.
    let thread = chat(&alice, "propose plan=update").await;
    let card = wait_entry(&alice, &thread, "proposal_card").await;
    let h = &card["handle"];
    // The default is the server's per-task limit, never a multiple of the model's estimate.
    assert_eq!((&h["title"], &h["apps"], &h["allow"], &h["period"], h["max_usd"].as_f64()), (&card["title"], &json!(["crm"]), &json!([]), &json!("week"), Some(50.0)), "{card}");
    assert!(card["your_apps"].as_array().is_some_and(|a| a.iter().any(|x| x["name"] == "crm")), "{card}");
    let id = card["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&w.app.api("bob"), &id, json!({ "decision": "handle" })).await.0, 404, "someone else handled the card");
    let (st, v) = decide(&alice, &id, json!({ "decision": "handle" })).await;
    assert_eq!(st, 200, "{v}");
    let (task, g) = (v["task"].as_str().expect("no task").to_string(), v["grant"].as_str().expect("no grant").to_string());
    assert_eq!(decide(&alice, &id, json!({ "decision": "handle" })).await.0, 409, "a card was handled twice");
    // Changing record 42 asks under the owner's policy; under the grant it goes ahead.
    went_ahead(&alice, &w.crm, "update_record", 0, &task).await;
    let card = wait_entry(&alice, &thread, "proposal_card").await;
    assert_eq!((&card["state"], &card["started"], &card["granted"]), (&json!("confirmed"), &json!(task), &json!(g)), "{card}");
    let gr = grant_of(&alice, &g).await;
    assert_eq!((&gr["title"], &gr["apps"], &gr["allow"], &gr["period"], gr["max_usd"].as_f64()), (&card["title"], &json!(["crm"]), &json!([]), &json!("week"), Some(50.0)), "{gr}");
    let cedar = gr["cedar"].as_str().unwrap();
    assert!(cedar.contains("permit(principal == Person::\"alice\"") && cedar.contains("resource in App::\"crm\"") && cedar.contains("resource.risk == \"financial\""), "{cedar}");
    assert_eq!(gr["still_asks"].as_array().map(|a| a.len()), Some(3), "{gr}");
    assert_eq!((&gr["actions"][0]["tool"], &gr["actions"][0]["task"]), (&json!("crm__update_record"), &json!(task)), "the action is not listed under the grant: {gr}");
    assert!(gr["spent"].as_f64().is_some_and(|s| s > 0.0), "{gr}");

    // Approving a proposal sets no grant: its task asks.
    let thread = chat(&alice, "propose plan=update").await;
    let card = wait_entry(&alice, &thread, "proposal_card").await;
    let (st, v) = decide(&alice, card["card"].as_str().unwrap(), json!({ "decision": "approve" })).await;
    assert_eq!(st, 200, "{v}");
    asked(&alice, Some(&w.crm), "update_record", v["task"].as_str().unwrap()).await;
    assert_eq!(grants(&alice).await.len(), 1, "approving a proposal set a grant");

    // A later task under the grant: Magnus knows it, and the task goes ahead.
    let later = run(&alice, "update", Some(&g)).await.expect("no task under the grant");
    went_ahead(&alice, &w.crm, "update_record", 1, &later).await;
    assert!(system(&after(&w.m, &format!("run plan=update grant={g}"))).contains(GRANTS), "the conversation does not know the grant");
    assert!(w.m.requests().iter().any(|c| system(c).contains(GRANTS) && system(c).contains(&g)), "the grant is not in the conversation's context");
    assert_eq!(grant_of(&alice, &g).await["actions"].as_array().map(|a| a.len()), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_and_what_cannot_be_undone_ask_until_lifted_and_money_and_the_rule_of_two_always_ask() {
    let w = world().await;
    let alice = w.app.api("alice");
    let g = grant(&alice, fields("Keep the CRM and the team current", &["crm", "team", "finance", "mail"], 5.0)).await;

    // Excluded by default: a post to the team (a send) and deleting.
    let post = run(&alice, "post", Some(&g)).await.unwrap();
    let item = asked(&alice, Some(&w.team), "post", &post).await;
    assert_eq!(item["details"]["tool"], "team__post");
    let delete = run(&alice, "delete", Some(&g)).await.unwrap();
    asked(&alice, Some(&w.crm), "delete_all", &delete).await;

    // Lifting sending on her behalf covers the post; deleting still asks until lifted too.
    let (st, v) = alice.put(&format!("/v1/grants/{g}"), json!({ "allow": ["send"] })).await;
    assert!(st.is_success(), "{v}");
    assert!(grant_of(&alice, &g).await["cedar"].as_str().unwrap().contains("irreversible"));
    let post = run(&alice, "post", Some(&g)).await.unwrap();
    went_ahead(&alice, &w.team, "post", 0, &post).await;
    let delete = run(&alice, "delete", Some(&g)).await.unwrap();
    asked(&alice, Some(&w.crm), "delete_all", &delete).await;
    assert!(alice.put(&format!("/v1/grants/{g}"), json!({ "allow": ["send", "irreversible"] })).await.0.is_success());
    let delete = run(&alice, "delete", Some(&g)).await.unwrap();
    went_ahead(&alice, &w.crm, "delete_all", 0, &delete).await;
    assert_eq!(grant_of(&alice, &g).await["still_asks"], json!(["spending money"]));

    // Spending money always asks, whatever is lifted, and cannot be lifted.
    let report = run(&alice, "report", Some(&g)).await.unwrap();
    asked(&alice, None, "get_report", &report).await;
    let (st, v) = alice.put(&format!("/v1/grants/{g}"), json!({ "allow": ["send", "spend"] })).await;
    assert_eq!(st.as_u16(), 400, "spending money was lifted: {v}");

    // The kernel's own asks are never covered: after reading mail written by others, a post asks.
    let both = run(&alice, "inbox-post", Some(&g)).await.unwrap();
    let item = asked(&alice, Some(&w.team), "post", &both).await;
    assert!(item["details"]["rule"].as_str().is_some_and(|r| r.starts_with("kernel:")), "{item}");
    assert_eq!(w.team.effects("post"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_whose_budget_for_the_period_is_spent_starts_no_task() {
    let w = world().await;
    let alice = w.app.api("alice");
    let g = grant(&alice, fields("Keep the CRM current", &["crm"], 5.0)).await;
    let first = run(&alice, "update", Some(&g)).await.unwrap();
    went_ahead(&alice, &w.crm, "update_record", 0, &first).await;
    let spent = grant_of(&alice, &g).await["spent"].as_f64().unwrap();
    assert!(spent > 0.0);
    // The budget, edited to below what was spent: the period's is used up.
    for bad in [0.0, 51.0] {
        assert_eq!(alice.put(&format!("/v1/grants/{g}"), json!({ "max_usd": bad })).await.0.as_u16(), 400, "max_usd {bad} was taken");
    }
    assert!(alice.put(&format!("/v1/grants/{g}"), json!({ "max_usd": spent / 2.0 })).await.0.is_success());
    assert_eq!(grant_of(&alice, &g).await["left"].as_f64(), Some(0.0));
    let before = alice.tasks().await["tasks"].as_array().unwrap().len();
    assert_eq!(run(&alice, "update", Some(&g)).await, None, "a task started under a spent grant");
    let turn = after(&w.m, &format!("run plan=update grant={g}"));
    let r = turn.last_results();
    assert!(r.iter().any(|r| r.is_error && r.text.contains("is spent")), "{r:?}");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), before);
    // Without the grant the work still starts, and asks.
    let plain = run(&alice, "update", None).await.unwrap();
    asked(&alice, Some(&w.crm), "update_record", &plain).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_is_its_persons_alone_and_revoked_in_settings_or_by_telling_magnus() {
    let w = world().await;
    let (alice, bob) = (w.app.api("alice"), w.app.api("bob"));
    let g = grant(&alice, fields("Keep the CRM current", &["crm"], 5.0)).await;

    // Bob cannot see, use, edit or revoke it.
    assert!(grants(&bob).await.is_empty(), "bob sees alice's grant");
    assert_eq!(bob.put(&format!("/v1/grants/{g}"), json!({ "max_usd": 1.0 })).await.0.as_u16(), 404);
    assert_eq!(bob.send(reqwest::Method::DELETE, &format!("/v1/grants/{g}"), None).await.0.as_u16(), 404);
    assert_eq!(run(&bob, "update", Some(&g)).await, None, "bob started a task under alice's grant");
    assert!(after(&w.m, &format!("run plan=update grant={g}")).last_results().iter().any(|r| r.is_error && r.text.contains("not a grant of this person's")));
    assert_eq!(w.crm.effects("update_record"), 0);

    // Revoked in Settings: no task starts under it.
    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/grants/{g}"), None).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(alice.send(reqwest::Method::DELETE, &format!("/v1/grants/{g}"), None).await.0.as_u16(), 409);
    assert_eq!(alice.put(&format!("/v1/grants/{g}"), json!({ "max_usd": 1.0 })).await.0.as_u16(), 409, "a revoked grant was edited");
    assert!(grant_of(&alice, &g).await["revoked"].is_string());
    let alices = w.m.requests().len();
    assert_eq!(run(&alice, "update", Some(&g)).await, None, "a task started under a revoked grant");
    assert!(w.m.requests()[alices..].iter().any(|c| c.last_results().iter().any(|r| r.is_error && r.text.contains("was revoked"))));

    // Revoked by telling Magnus.
    let g2 = grant(&alice, fields("Keep the CRM tidy", &["crm"], 5.0)).await;
    chat(&alice, &format!("revoke grant={g2}")).await;
    assert!(grant_of(&alice, &g2).await["revoked"].is_string(), "revoke_grant left it live");
    assert_eq!(run(&alice, "update", Some(&g2)).await, None);
    // Bob cannot revoke Alice's by telling Magnus either.
    let g3 = grant(&alice, fields("Keep the CRM in order", &["crm"], 5.0)).await;
    chat(&bob, &format!("revoke grant={g3}")).await;
    assert!(grant_of(&alice, &g3).await["revoked"].is_null(), "bob revoked alice's grant");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_card_is_accepted_with_the_persons_changes_or_declined_with_a_reason() {
    let w = world().await;
    let (alice, bob) = (w.app.api("alice"), w.app.api("bob"));

    // A card that names an app she does not have, or lifts spending, is refused.
    for (bad, why) in [(json!({ "apps": ["nowhere"] }), "not one of the person's apps"), (json!({ "allow": ["spend"] }), "spending money always asks")] {
        let mut f = fields("Post the team's status", &["team"], 3.0);
        for (k, v) in bad.as_object().unwrap() {
            f[k] = v.clone();
        }
        let text = format!("grant:{f}");
        let thread = chat(&alice, &text).await;
        assert!(!entries(&alice, &thread).await.iter().any(|e| e["kind"] == "grant_card"), "a bad grant card was put");
        assert!(after(&w.m, &text).last_results().iter().any(|r| r.is_error && r.text.contains(why)), "{why}");
    }

    let thread = chat(&alice, &format!("grant:{}", fields("Post the team's status (grant-dec-3)", &["team"], 3.0))).await;
    let card = wait_entry(&alice, &thread, "grant_card").await;
    assert_eq!((&card["state"], &card["apps"], card["max_usd"].as_f64(), &card["period"]), (&json!("open"), &json!(["team"]), Some(3.0), &json!("week")), "{card}");
    assert_eq!(card["still_asks"].as_array().map(|a| a.len()), Some(3), "{card}");
    let id = card["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&bob, &id, json!({ "decision": "accept" })).await.0, 404, "someone else accepted it");
    assert_eq!(decide(&alice, &id, json!({ "decision": "later" })).await.0, 400, "a grant card was put off");
    assert_eq!(decide(&alice, &id, json!({ "decision": "decline" })).await.0, 400, "declined without a reason");
    assert_eq!(decide(&alice, &id, json!({ "decision": "decline", "reason": "I post those myself (why-g-9)." })).await.0, 200);
    let card = wait_entry(&alice, &thread, "grant_card").await;
    assert_eq!((&card["state"], &card["reason"]), (&json!("declined"), &json!("I post those myself (why-g-9).")), "{card}");
    assert!(grants(&alice).await.is_empty(), "a declined card set a grant");
    // The next conversation reads the decline in the ledger.
    chat(&alice, "Hello.").await;
    let sys = system(w.m.requests().iter().rev().find(|c| said(c) == "Hello.").unwrap());
    assert!(sys.contains(LEDGER) && sys.contains("you proposed the grant") && sys.contains("why-g-9"), "{sys}");

    // Accepted with her changes: those are the grant.
    let thread = chat(&alice, &format!("grant:{}", fields("Post the team's status", &["team"], 3.0))).await;
    let card = wait_entry(&alice, &thread, "grant_card").await;
    let (st, v) = decide(&alice, card["card"].as_str().unwrap(), json!({ "decision": "accept", "max_usd": 2.0, "period": "day", "allow": ["send"] })).await;
    assert_eq!(st, 200, "{v}");
    let g = v["grant"].as_str().unwrap().to_string();
    let gr = grant_of(&alice, &g).await;
    assert_eq!((gr["max_usd"].as_f64(), &gr["period"], &gr["allow"], &gr["title"]), (Some(2.0), &json!("day"), &json!(["send"]), &json!("Post the team's status")), "{gr}");
    let card = wait_entry(&alice, &thread, "grant_card").await;
    assert_eq!((&card["state"], &card["granted"]), (&json!("confirmed"), &json!(g)), "{card}");

    // A card that changes it keeps its id.
    let mut f = fields("Post the team's status and the CRM's", &["team", "crm"], 4.0);
    f["replaces"] = json!(g);
    let thread = chat(&alice, &format!("grant:{f}")).await;
    let card = wait_entry(&alice, &thread, "grant_card").await;
    let (st, v) = decide(&alice, card["card"].as_str().unwrap(), json!({ "decision": "accept" })).await;
    assert_eq!((st, &v["grant"]), (200, &json!(g)), "{v}");
    let all = grants(&alice).await;
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!((&all[0]["apps"], &all[0]["title"]), (&json!(["team", "crm"]), &json!("Post the team's status and the CRM's")));
    assert!(all[0]["edited"].is_string());
}

/// A grant's budget holds across its tasks together: a task already running
/// under it stops at its next model call once the grant's budget for the
/// period is spent, until its person resumes it; and deleting a task under
/// it gives none of what it spent back (review of #278, #280).
#[tokio::test(flavor = "multi_thread")]
async fn a_grants_budget_holds_across_its_tasks_and_deleting_one_gives_nothing_back() {
    let w = world().await;
    let alice = w.app.api("alice");
    let g = grant(&alice, fields("Keep the CRM current", &["crm"], 5.0)).await;
    let first = run(&alice, "update", Some(&g)).await.unwrap();
    went_ahead(&alice, &w.crm, "update_record", 0, &first).await;
    let spent = grant_of(&alice, &g).await["spent"].as_f64().unwrap();
    assert!(spent > 0.0);

    // Deleted, and removed for good: what it spent stays spent.
    let (st, v) = alice.post("/v1/trash", json!({ "item": first })).await;
    assert!(st.is_success(), "{v}");
    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{first}"), None).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(grant_of(&alice, &g).await["spent"].as_f64(), Some(spent), "deleting a task gave its cost back to the grant");

    // A task under it waits on an approval (the team chat is not in the
    // grant) while the grant's budget runs out: it was started with what was
    // left then, but goes no further once the grant's is spent.
    let second = run(&alice, "post", Some(&g)).await.unwrap();
    let item = asked(&alice, Some(&w.team), "post", &second).await;
    let now = grant_of(&alice, &g).await["spent"].as_f64().unwrap();
    assert!(alice.put(&format!("/v1/grants/{g}"), json!({ "max_usd": now / 2.0 })).await.0.is_success());
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("the approved post ran", PATIENCE, || async { (w.team.effects("post") == 1).then_some(()) }).await;
    let stop = alice.wait_item(&second, "notice").await;
    assert_eq!(stop["details"]["reason"], "grant_budget", "{stop}");
    let calls = |m: &FakeModel| m.requests().into_iter().filter(|c| c.text().contains("plan=post") && !system(c).contains(CHAT)).count();
    let before = calls(&w.m);
    never("the task asked the model past its grant's budget", Duration::from_secs(2), || async { calls(&w.m) > before }).await;

    // Its person resumes it: theirs to decide, it goes on under its own budget.
    assert!(alice.respond(stop["id"].as_str().unwrap(), json!({ "decision": "resume" })).await.is_success());
    eventually("the resumed task asked the model again", PATIENCE, || async { (calls(&w.m) > before).then_some(()) }).await;
    alice.wait_settled(&second).await;
    assert!(alice.open_items(&second, "notice").await.iter().all(|n| n["details"]["reason"] != "grant_budget"), "it stopped again after its person resumed it");
}

/// Once a grant's budget for the period is spent, no paid call of a task
/// under it goes out: not the verifier's (here, its review of the contract
/// the spending reply recorded) any more than the agent's (review of #284,
/// #286).
#[tokio::test(flavor = "multi_thread")]
async fn a_spent_grant_stops_the_verifiers_calls_too() {
    let w = world_with(true).await;
    let alice = w.app.api("alice");
    // Its first reply spends all of it.
    let g = grant(&alice, fields("Keep the CRM current", &["crm"], 0.000001)).await;
    let t = run(&alice, "update", Some(&g)).await.expect("no task started");
    // It stops for its budget: the run's own is what was left of the grant's.
    let stop = alice.wait_item(&t, "notice").await;
    assert!(matches!(stop["details"]["reason"].as_str(), Some("grant_budget" | "budget")), "{stop}");
    let reviews = w.m.requests().into_iter().filter(|c| system(c).contains("You review a task's acceptance criteria")).count();
    assert_eq!(reviews, 0, "the verifier was asked after the grant's budget was spent");
    assert_eq!(w.crm.effects("update_record"), 0);
}
