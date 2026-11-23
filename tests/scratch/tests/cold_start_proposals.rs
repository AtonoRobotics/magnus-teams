//! Openers (behavior spec, Suggestions and proposals: "at cold start, and
//! for each newly connected app, Magnus looks at what it may see and
//! proposes two or three things it could take over"; behavior.md step 4;
//! #255), as they are since 2026-11-21 (`decisions/2026-11-21-ideas-go-to-explore.md`,
//! owner: "Put them in Explore, no new threads"). A person's first sign-in
//! with nothing here yet, and their first connection of an app, each start
//! one run of Magnus's own, as a review is. It opens no conversation: its
//! suggestions cite what it read and wait as ideas for Explore's For you,
//! where they are started or dismissed. The person is told once that they
//! are there. Nothing opens twice (a second sign-in, a restart, a
//! reconnection), and nothing opens while the kill switch is on.

use magnus_invariants::*;
use magnus_scratch::{reconfigure, set_password, FakeOAuthApp, PASSWORD};
use serde_json::{json, Value};
use std::time::Duration;

const PASS: &str = "You keep the person's long-term memory";
/// From the review's instructions (`instructions/review.md`).
const REVIEW: &str = "reviewing one person's state on your own initiative";
/// From the opener's instruction (`instructions/opener.md`, `opener-app.md`).
const OPENS: &str = "nobody has written yet";
const SUGGESTED: &str = "Sort your week (open-sug-12)";
const PROPOSED: &str = "Chase the overdue invoice (open-prop-4)";
const GOAL: &str = "Each Monday, lay out the week.";

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

/// The id of this run's call of `tool`.
fn call_of(c: &Conversation, tool: &str) -> Value {
    let blocks = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default());
    blocks.into_iter().find(|b| b["type"] == "tool_use" && b["name"] == tool).map(|b| b["id"].clone()).unwrap_or_else(|| panic!("no {tool} call"))
}

fn opening(c: &Conversation) -> bool {
    said(c).contains(OPENS)
}

/// An opener reads (the app's events, or the person's schedules), puts two
/// suggestions citing that read and finishes; a chat turn answers in a
/// line; a task plans and ends; the memory pass writes nothing.
fn model() -> model::Script {
    script(|c| async move {
        let sys = system(&c);
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !opening(&c) {
            return match c.turn() {
                0 if sys.contains(REVIEW) => complete("Nothing to say."),
                0 => plan("do the work"),
                _ => fail("done"),
            };
        }
        let read = if said(&c).contains("Calendar") { "calendar__list_events" } else { "list_schedules" };
        let evidence = || json!([{ "call": call_of(&c, read), "shows": "What is there now." }]);
        match c.turn() {
            0 => tool(read, json!({})),
            1 => tool("suggest", json!({ "title": SUGGESTED, "goal": GOAL, "evidence": evidence(), "permissions": ["Reads your calendar", "Sends nothing"] })),
            2 => tool(
                "suggest",
                json!({ "title": PROPOSED, "goal": "Draft a reminder to Acme about invoice 1042.", "evidence": evidence(), "permissions": ["Drafts an email; sends nothing"] }),
            ),
            _ => complete("Suggested two things."),
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

/// The person's conversations.
async fn conversations(api: &Api) -> Vec<Value> {
    let (st, v) = api.get("/v1/chat").await;
    assert!(st.is_success(), "{v}");
    v["threads"].as_array().cloned().unwrap_or_default()
}

/// The person's ideas still waiting, oldest first.
async fn ideas(api: &Api) -> Vec<Value> {
    let mut v: Vec<Value> = api.inbox().await.into_iter().filter(|i| i["kind"] == "suggestion" && i["status"] == "open").collect();
    v.sort_by_key(|i| i["created_at"].as_str().unwrap_or_default().to_string());
    v
}

/// The notices that ideas are waiting in Explore.
async fn told(api: &Api) -> Vec<Value> {
    api.inbox().await.into_iter().filter(|i| i["kind"] == "notice" && i["details"]["reason"] == "ideas").collect()
}

/// Wait for an opener's two ideas and its one notice.
async fn the_ideas(api: &Api) -> Vec<Value> {
    eventually("Magnus's ideas and the notice that they are in Explore", PATIENCE, || async {
        let (i, t) = (ideas(api).await, told(api).await);
        (i.len() == 2 && t.len() == 1).then_some(i)
    })
    .await
}

/// Wait for the task an idea started.
async fn started(api: &Api) -> Value {
    eventually("the started idea's task", PATIENCE, || async { api.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == GOAL).cloned() }).await
}

/// Wait for an opener to have reached the model.
async fn ran(m: &FakeModel) {
    eventually("an opener", PATIENCE, || async { m.requests().into_iter().any(|c| opening(&c)).then_some(()) }).await
}

fn titled<'a>(list: &'a [Value], title: &str) -> &'a Value {
    list.iter().find(|i| i["details"]["title"] == title).unwrap_or_else(|| panic!("no idea {title} in {list:?}"))
}

/// Long enough for a trigger that was going to start to have reached the model.
async fn a_moment() {
    tokio::time::sleep(Duration::from_secs(3)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_sign_in_opens_no_conversation_and_its_ideas_wait_in_explore() {
    let m = FakeModel::start(model()).await;
    let mut app = start(&m).await;
    let carol = app.api("carol");
    assert!(set_password(&app, "carol", PASSWORD));
    sign_in(&app, "carol").await;
    let list = the_ideas(&carol).await;

    // No conversation, and Magnus's own run, under the review's instructions.
    assert!(conversations(&carol).await.is_empty(), "the opener opened a conversation: {:?}", conversations(&carol).await);
    let run = m.requests().into_iter().find(opening).expect("the opener reached the model");
    assert!(system(&run).contains(REVIEW), "the opener ran as a chat turn");
    assert!(!carol.tasks().await["tasks"].as_array().unwrap().iter().any(|t| t["review"] != true), "the opener is listed as her task");
    // Its ideas cite what it read, and are hers alone.
    for i in &list {
        assert_eq!((&i["details"]["evidence"][0]["tool"], &i["person"]), (&json!("list_schedules"), &json!("carol")), "{i}");
    }
    assert!(ideas(&app.api("alice")).await.is_empty(), "another person has carol's ideas");
    // One notice says where they are, and nothing else is in front of her.
    let notice = &told(&carol).await[0];
    assert_eq!((&notice["summary"], &notice["details"]["ideas"]), (&json!("Magnus has 2 ideas for you in Explore"), &json!(2)), "{notice}");
    let other: Vec<Value> = carol.inbox().await.into_iter().filter(|i| i["status"] == "open" && i["kind"] != "suggestion" && i["details"]["reason"] != "ideas").collect();
    assert!(other.is_empty(), "the opener put more in front of her: {other:?}");

    // Starting one starts its task for her; dismissing the other leaves none.
    let path = |i: &Value| format!("/v1/inbox/{}", i["id"].as_str().unwrap());
    let (st, v) = carol.post(&path(titled(&list, SUGGESTED)), json!({ "decision": "accept" })).await;
    assert!(st.is_success(), "{v}");
    started(&carol).await;
    assert!(!m.requests().into_iter().filter(|c| said(c).contains(GOAL)).any(|c| c.text().contains(OPENS)), "the task was given the opener's instruction");
    let (st, v) = carol.post(&path(titled(&list, PROPOSED)), json!({ "decision": "reject" })).await;
    assert!(st.is_success(), "{v}");
    assert!(ideas(&carol).await.is_empty());

    // Signing in again, and after a restart, runs nothing more and tells her nothing more.
    let turns = m.requests().into_iter().filter(opening).count();
    sign_in(&app, "carol").await;
    app.restart().await;
    sign_in(&app, "carol").await;
    a_moment().await;
    assert_eq!(m.requests().into_iter().filter(opening).count(), turns, "the opener ran again");
    assert_eq!(told(&carol).await.len(), 1, "she was told again");
    assert!(conversations(&carol).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn connecting_an_app_the_first_time_reads_it_and_leaves_ideas_and_reconnecting_does_not() {
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
    // Alice connected it first, before its tools were classified: hers ran too.
    ran(&m).await;

    let before = cal.calls("list_events");
    connect(&carol, port).await;
    let list = the_ideas(&carol).await;
    assert!(conversations(&carol).await.is_empty(), "connecting opened a conversation");
    // Its run read the app, and its ideas cite that read.
    assert_eq!(cal.calls("list_events"), before + 1, "the opener did not read the app");
    let sug = titled(&list, SUGGESTED);
    assert!(sug["details"]["evidence"][0]["tool"].as_str().unwrap_or_default().contains("list_events"), "{sug}");

    // Disconnected and connected again: nothing new runs.
    let turns = m.requests().into_iter().filter(opening).count();
    let (st, v) = carol.send(reqwest::Method::DELETE, "/v1/connectors/calendar/connection", None).await;
    assert!(st.is_success(), "{v}");
    connect(&carol, port).await;
    a_moment().await;
    assert_eq!(m.requests().into_iter().filter(opening).count(), turns, "an opener ran again on reconnecting");
    assert_eq!(cal.calls("list_events"), before + 1);
    assert_eq!(told(&carol).await.len(), 1);
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
    assert_eq!(m.requests().into_iter().filter(opening).count(), 0, "an opener reached the model under the kill switch");

    // Released, her next sign-in is still her cold start: she has nothing here yet.
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
    sign_in(&app, "carol").await;
    the_ideas(&carol).await;
}
