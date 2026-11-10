//! Openers (behavior spec, Suggestions and proposals: "at cold start, and
//! for each newly connected app, Magnus looks at what it may see and
//! proposes two or three things it could take over"; behavior.md step 4;
//! #255). A person's first sign-in with nothing here yet, and their first
//! connection of an app, each start one chat turn Magnus opens itself, in a
//! conversation of its own: its cards cite what the turn read and are
//! answered like any card. Its text is the appliance's instruction, never
//! shown or transcribed as the person's message. Nothing opens twice (a
//! second sign-in, a restart, a reconnection), and nothing opens while the
//! kill switch is on. The empty Tasks and Schedules list the cards still
//! waiting, from the conversation list.

use magnus_invariants::*;
use magnus_scratch::{reconfigure, set_password, FakeOAuthApp, PASSWORD};
use serde_json::{json, Value};
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
/// From the opener's instruction (`instructions/opener.md`, `opener-app.md`).
const OPENS: &str = "nobody has written yet";
const SUGGESTED: &str = "Sort your week (open-sug-12)";
const PROPOSED: &str = "Chase the overdue invoice (open-prop-4)";

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

fn opening(c: &Conversation) -> bool {
    system(c).contains(CHAT) && said(c).contains(OPENS)
}

/// An opener reads (the app's events, or the person's tasks) and puts two
/// suggestions citing that read; any other chat turn
/// answers in a line; a task states a contract and ends; the memory pass
/// writes nothing.
fn model() -> model::Script {
    script(|c| async move {
        let sys = system(&c);
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !sys.contains(CHAT) {
            return match c.turn() {
                0 => contract_unchecked("do the work", "it is done"),
                _ => fail("done"),
            };
        }
        if !said(&c).contains(OPENS) {
            return Reply::Text("Noted.".into());
        }
        let read = if said(&c).contains("Calendar") { "calendar__list_events" } else { "list_tasks" };
        let evidence = || json!([{ "call": call_of(&c, read), "shows": "What is there now." }]);
        match c.turn() {
            0 => tool(read, json!({})),
            1 => tool(
                "suggest",
                json!({ "title": SUGGESTED, "goal": "Each Monday, lay out the week.", "evidence": evidence(), "permissions": ["Reads your calendar", "Sends nothing"] }),
            ),
            2 => tool(
                "suggest",
                json!({ "title": PROPOSED, "goal": "Draft a reminder to Acme about invoice 1042.", "evidence": evidence(), "permissions": ["Drafts an email; sends nothing"] }),
            ),
            _ => Reply::Text("I'm Magnus. I had a look around; two cards await you.".into()),
        }
    })
}

/// An appliance with openers on, whatever the test harness's default.
async fn start(m: &FakeModel) -> Appliance {
    let mut app = Appliance::start(Spec::default(), m).await;
    reconfigure(&mut app, |t| {
        t.insert("proposals".into(), toml::Value::Table([("openers".to_string(), toml::Value::Boolean(true))].into_iter().collect()));
    })
    .await;
    app
}

async fn sign_in(app: &Appliance, who: &str) {
    let r = reqwest::Client::new().post(format!("{}/v1/sign-in", app.base)).json(&json!({ "person": who, "password": PASSWORD })).send().await.unwrap();
    assert_eq!(r.status(), 200, "{who} could not sign in: {}", r.text().await.unwrap_or_default());
}

/// The person's conversations Magnus opened.
async fn opened(api: &Api) -> Vec<Value> {
    let (st, v) = api.get("/v1/chat").await;
    assert!(st.is_success(), "{v}");
    v["threads"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["opener"].is_object()).collect()
}

/// Wait for the one conversation Magnus opened, its turn settled.
async fn the_opened(api: &Api) -> (Value, Vec<Value>) {
    let t = eventually("Magnus opens a conversation", PATIENCE, || async {
        let o = opened(api).await;
        (o.len() == 1).then(|| o[0].clone())
    })
    .await;
    let thread = t["thread"].as_str().unwrap();
    let opener = entries(api, thread).await.into_iter().find(|e| e["kind"] == "opener").unwrap_or_else(|| panic!("no opener entry in {thread}"));
    api.wait_settled(opener["task_id"].as_str().unwrap()).await;
    (opened(api).await.into_iter().find(|o| o["thread"] == thread).unwrap(), entries(api, thread).await)
}

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/chat/{thread}")).await;
    assert!(st.is_success(), "{v}");
    v["entries"].as_array().cloned().unwrap_or_default()
}

fn card<'a>(es: &'a [Value], title: &str) -> &'a Value {
    es.iter().find(|e| e["kind"] == "suggestion_card" && e["title"] == title).unwrap_or_else(|| panic!("no card {title} in {es:?}"))
}

async fn decide(api: &Api, card: &Value, body: Value) -> Value {
    let (st, v) = api.post(&format!("/v1/chat/cards/{}", card["card"].as_str().unwrap()), body).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    v
}

/// What the empty Tasks and Schedules offer: the opened conversations' cards still waiting.
async fn offered(api: &Api) -> Vec<String> {
    opened(api).await.iter().flat_map(|t| t["open_cards"].as_array().cloned().unwrap_or_default()).map(|c| c["title"].as_str().unwrap_or_default().to_string()).collect()
}

/// Long enough for a trigger that was going to start to have reached the model.
async fn a_moment() {
    tokio::time::sleep(Duration::from_secs(3)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_sign_in_opens_one_conversation_whose_cards_cite_what_it_read_and_are_answered_like_any() {
    let m = FakeModel::start(model()).await;
    let mut app = start(&m).await;
    let carol = app.api("carol");
    assert!(set_password(&app, "carol", PASSWORD));
    sign_in(&app, "carol").await;
    let (t, es) = the_opened(&carol).await;
    assert_eq!((&t["title"], &t["opener"]["app"]), (&json!("What I could take over"), &Value::Null), "{t}");

    // Magnus's own opening, never a message of hers, and the instruction is nowhere in it.
    assert_eq!(es[0]["kind"], "opener", "{es:?}");
    assert!(!es.iter().any(|e| e["kind"] == "message"), "the opener shows as the person's message: {es:?}");
    assert!(!json!(es).to_string().contains(OPENS), "the thread shows the opener's instruction: {es:?}");
    // Its cards cite what its turn read.
    let (sug, prop) = (card(&es, SUGGESTED).clone(), card(&es, PROPOSED).clone());
    for c in [&sug, &prop] {
        assert_eq!((&c["state"], &c["evidence"][0]["tool"], &c["acts_for"]), (&json!("open"), &json!("list_tasks"), &json!("carol")), "{c}");
    }
    assert_eq!(offered(&carol).await, vec![SUGGESTED.to_string(), PROPOSED.to_string()]);
    assert!(offered(&app.api("alice")).await.is_empty(), "another person is offered carol's cards");

    // Accepting the suggestion starts a task for her, which is not told the instruction was hers.
    let v = decide(&carol, &sug, json!({ "decision": "accept" })).await;
    let task = carol.wait_settled(v["task"].as_str().expect("no task started")).await;
    assert_eq!(task["person"], "carol", "{task}");
    let tasks: Vec<Conversation> = m.requests().into_iter().filter(|c| !system(c).contains(CHAT) && !system(c).contains(PASS)).collect();
    assert!(tasks.iter().any(|c| c.text().contains("Each Monday, lay out the week.")), "the accepted card's task did not run");
    assert!(!tasks.iter().any(|c| c.text().contains(OPENS)), "the task was given the opener's instruction");
    // An answered card leaves the empty-state list; dismissing the other empties it.
    assert_eq!(offered(&carol).await, vec![PROPOSED.to_string()]);
    decide(&carol, &prop, json!({ "decision": "dismiss", "reason": "Acme paid." })).await;
    assert!(offered(&carol).await.is_empty());

    // She answers in it: her turn reads the opening as Magnus's, never as what she said.
    let thread = t["thread"].as_str().unwrap();
    let (st, v) = carol.post("/v1/chat", json!({ "thread": thread, "text": "Thanks (reply-77)" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    let reply = m.requests().into_iter().find(|c| system(c).contains(CHAT) && said(c).contains("reply-77")).expect("her turn reached the model");
    assert!(reply.text().contains("You opened this conversation yourself"), "{}", reply.text());
    assert!(!reply.text().contains(OPENS), "her turn was given the opener's instruction as part of the conversation");
    // Neither the opening nor its answer goes again as a message of hers.
    let own: Vec<&Value> = es.iter().filter(|e| e["task_id"] == es[0]["task_id"] && matches!(e["kind"].as_str(), Some("opener" | "answer"))).collect();
    assert_eq!(own.len(), 2, "{es:?}");
    for e in own {
        let (st, v) = carol.post(&format!("/v1/chat/{thread}/entries/{}/resend", e["id"].as_str().unwrap()), json!({})).await;
        assert_eq!(st.as_u16(), 400, "the opening was sent again as her message: {e} {v}");
    }

    // Signing in again, and after a restart, opens nothing more.
    let turns = m.requests().into_iter().filter(opening).count();
    sign_in(&app, "carol").await;
    app.restart().await;
    sign_in(&app, "carol").await;
    a_moment().await;
    assert_eq!(opened(&carol).await.len(), 1, "a second conversation opened");
    assert_eq!(m.requests().into_iter().filter(opening).count(), turns, "the opener ran again");
}

#[tokio::test(flavor = "multi_thread")]
async fn connecting_an_app_the_first_time_opens_one_conversation_that_reads_it_and_reconnecting_does_not() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = FakeModel::start(model()).await;
    let app = start(&m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    connect(&alice, port).await;
    assert_eq!(alice.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "read" })).await.0, 204);
    // Alice connected it first, before its tools were classified: hers opened too.
    let (t, _) = the_opened(&alice).await;
    assert_eq!(t["opener"]["app"], "calendar", "{t}");

    let before = cal.calls("list_events");
    connect(&carol, port).await;
    let (t, es) = the_opened(&carol).await;
    assert_eq!((&t["title"], &t["opener"]["app"]), (&json!("What I could take over in Calendar"), &json!("calendar")), "{t}");
    let o = &es[0];
    assert_eq!((&o["kind"], &o["app"]), (&json!("opener"), &json!("calendar")), "{es:?}");
    assert!(!json!(es).to_string().contains(OPENS), "the thread shows the opener's instruction: {es:?}");
    // Its run read the app, and its cards cite that read.
    assert_eq!(cal.calls("list_events"), before + 1, "the opener did not read the app");
    let sug = card(&es, SUGGESTED);
    assert!(sug["evidence"][0]["tool"].as_str().unwrap_or_default().contains("list_events"), "{sug}");
    assert_eq!(offered(&carol).await.len(), 2);

    // Disconnected and connected again: nothing new opens.
    let turns = m.requests().into_iter().filter(opening).count();
    let (st, v) = carol.send(reqwest::Method::DELETE, "/v1/connectors/calendar/connection", None).await;
    assert!(st.is_success(), "{v}");
    connect(&carol, port).await;
    a_moment().await;
    assert_eq!(opened(&carol).await.len(), 1, "reconnecting opened another conversation");
    assert_eq!(m.requests().into_iter().filter(opening).count(), turns, "an opener ran again on reconnecting");
    assert_eq!(cal.calls("list_events"), before + 1);
}

/// The person signs in to the app (MCP's authorization), the code coming
/// back to a loopback address on their computer.
async fn connect(api: &Api, port: u16) {
    let (st, v) = api.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(api.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_opens_while_the_kill_switch_is_on() {
    let m = FakeModel::start(model()).await;
    let app = start(&m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    assert!(set_password(&app, "carol", PASSWORD));
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    sign_in(&app, "carol").await;
    a_moment().await;
    assert!(opened(&carol).await.is_empty(), "a conversation opened under the kill switch");
    assert_eq!(m.requests().into_iter().filter(opening).count(), 0, "an opener reached the model under the kill switch");

    // Released, her next sign-in is still her cold start: she has nothing here yet.
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
    sign_in(&app, "carol").await;
    let (t, _) = the_opened(&carol).await;
    assert_eq!(t["title"], "What I could take over", "{t}");
}

/// Deleting the conversation Magnus opened neither starts it again nor shows
/// its instruction in the trash: it is listed by its title, and a later
/// sign-in, a restart and removing it for good open nothing (review of #274).
#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_opener_conversation_is_listed_by_its_title_and_does_not_open_again() {
    let m = FakeModel::start(model()).await;
    let mut app = start(&m).await;
    let carol = app.api("carol");
    assert!(set_password(&app, "carol", PASSWORD));
    sign_in(&app, "carol").await;
    let (t, _) = the_opened(&carol).await;
    let thread = t["thread"].as_str().unwrap().to_string();
    let turns = m.requests().into_iter().filter(opening).count();

    let (st, v) = carol.post("/v1/trash", json!({ "item": thread })).await;
    assert!(st.is_success(), "{v}");
    let (st, trash) = carol.get("/v1/trash").await;
    assert!(st.is_success(), "{trash}");
    let held = trash["items"].as_array().unwrap().iter().find(|i| i["item"] == thread.as_str()).cloned().unwrap_or_else(|| panic!("not in the trash: {trash}"));
    assert_eq!(held["title"], "What I could take over", "{held}");
    assert!(!trash.to_string().contains(OPENS), "the trash shows the opener's instruction: {trash}");

    sign_in(&app, "carol").await;
    a_moment().await;
    app.restart().await;
    sign_in(&app, "carol").await;
    a_moment().await;
    let (st, v) = carol.send(reqwest::Method::DELETE, &format!("/v1/trash/{thread}"), None).await;
    assert!(st.is_success(), "{v}");
    app.restart().await;
    sign_in(&app, "carol").await;
    a_moment().await;
    assert!(opened(&carol).await.is_empty(), "a conversation opened again");
    assert_eq!(m.requests().into_iter().filter(opening).count(), turns, "the opener ran again after its conversation was deleted");
}

/// Her cold-start opening takes reactions like any message of Magnus's:
/// accepted, shown on the thread, and carried into her later runs (#359).
#[tokio::test(flavor = "multi_thread")]
async fn the_opener_takes_reactions() {
    let m = FakeModel::start(model()).await;
    let app = start(&m).await;
    let carol = app.api("carol");
    assert!(set_password(&app, "carol", PASSWORD));
    sign_in(&app, "carol").await;
    let (t, es) = the_opened(&carol).await;
    let thread = t["thread"].as_str().unwrap();
    let entry = es[0]["id"].as_str().unwrap().to_string();
    assert_eq!(es[0]["kind"], "opener", "{es:?}");

    let (st, v) = carol.post(&format!("/v1/chat/{thread}/entries/{entry}/reaction"), json!({ "reaction": "annoying" })).await;
    assert!(st.is_success(), "{v}");

    let es = entries(&carol, thread).await;
    assert_eq!(es[0]["reaction"], json!("annoying"), "{es:?}");

    // Her next run reads it beside the others.
    let (st, v) = carol.post("/v1/chat", json!({ "text": "What else is on?" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    let sys = m.requests().into_iter().find(|c| c.turn() == 0 && said(c) == "What else is on?").map(|c| system(&c)).unwrap_or_else(|| panic!("no model call for her message"));
    assert!(sys.contains("that was annoying"), "{sys}");
}
