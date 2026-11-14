//! Magnus knows the person's apps while it works (connectors spec, How the
//! agent uses connectors; Inside work):
//! - every chat and task holds the person's app index: each app their
//!   organisation lets them use, with whether they have connected it, and
//!   none it does not;
//! - asked to do something an unconnected app does, Magnus finds it in their
//!   Library by what is wanted, and offers its card in the conversation
//!   with why; the card shows Connect, and nothing is connected until the
//!   person signs in;
//! - it cannot offer an app that is not in the person's Library, and says
//!   so when nothing fits;
//! - an app that refuses Magnus its tools is said to, from the first model
//!   call, so Magnus tells the person rather than trying another way.

use magnus_invariants::model::Script;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const CHAT: &str = "talking with the person you work for in a chat";

/// Each chat turn calls the tools listed for what the person said, one per
/// model call, then answers with the last result.
fn model(plan: Arc<Mutex<Vec<(String, Vec<(String, Value)>)>>>, seen: Arc<Mutex<Vec<(Value, Vec<String>)>>>) -> Script {
    script(move |c| {
        let (plan, seen) = (plan.clone(), seen.clone());
        async move {
            if !c.body["system"].to_string().contains(CHAT) {
                return complete("done");
            }
            seen.lock().unwrap().push((c.body.clone(), c.last_results().iter().map(|r| r.text.clone()).collect()));
            let first = c.messages().first().cloned().unwrap_or_default();
            let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            let steps = plan.lock().unwrap().iter().find(|(s, _)| *s == said).map(|(_, t)| t.clone()).unwrap_or_default();
            match steps.get(c.turn()) {
                Some((t, input)) => tool(t, input.clone()),
                None => Reply::Text(c.last_results().first().map(|r| r.text.clone()).unwrap_or_else(|| "Done.".into())),
            }
        }
    })
}

/// Say `text` in a new conversation and wait for the answer: the thread's
/// id and its entries.
async fn say(api: &Api, plan: &Arc<Mutex<Vec<(String, Vec<(String, Value)>)>>>, text: &str, steps: Vec<(&str, Value)>) -> (String, Vec<Value>) {
    plan.lock().unwrap().push((text.into(), steps.into_iter().map(|(t, i)| (t.to_string(), i)).collect()));
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    let entries = eventually("the answer", PATIENCE, || async {
        let (_, t) = api.get(&format!("/v1/chat/{thread}")).await;
        let e = t["entries"].as_array().cloned().unwrap_or_default();
        e.iter().any(|x| x["kind"] == "answer").then_some(e)
    })
    .await;
    (thread, entries)
}

/// What the tools said back in this conversation's turns so far.
fn said_back(seen: &Arc<Mutex<Vec<(Value, Vec<String>)>>>) -> String {
    seen.lock().unwrap().iter().flat_map(|(_, r)| r.clone()).collect::<Vec<_>>().join("\n")
}

/// The opening message of the first request.
fn opening(seen: &Arc<Mutex<Vec<(Value, Vec<String>)>>>) -> String {
    seen.lock().unwrap().first().map(|(b, _)| b["messages"][0]["content"].to_string()).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn magnus_finds_an_unconnected_app_by_what_is_wanted_and_offers_its_card_with_why() {
    let plan = Arc::new(Mutex::new(vec![]));
    let seen: Arc<Mutex<Vec<(Value, Vec<String>)>>> = Arc::new(Mutex::new(vec![]));
    let m = FakeModel::start(model(plan.clone(), seen.clone())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let carol = app.api("carol");
    assert!(magnus_scratch::set_password(&app, "carol", magnus_scratch::PASSWORD));
    // The organisation's calendar, added by its address: ready to connect.
    let cal = magnus_scratch::FakeOAuthApp::start(3600).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let name = v["name"].as_str().unwrap().to_string();

    // The calendar is in Carol's Library, not connected; she asks for a time.
    let (id, thread) = say(
        &carol,
        &plan,
        "Find a time with my team next week.",
        vec![("find_apps", json!({ "query": "find a time on my calendar" })), ("offer_app", json!({ "name": name, "why": "To find a time your team is free" }))],
    )
    .await;

    // The index was in the context, before her words.
    let opening = opening(&seen);
    assert!(
        opening.contains("The person's apps") && opening.contains(&format!("({name})")) && opening.contains("(google-calendar)") && opening.contains("Not connected."),
        "no app index in the context: {opening}"
    );
    // find_apps found the calendars by what was wanted.
    let back = said_back(&seen);
    assert!(back.contains(&format!("\"name\":\"{name}\"")) && back.contains("google-calendar"), "find_apps did not find the calendars: {back}");

    // Its card is in the conversation with why, and offers Connect.
    let card = thread.iter().find(|e| e["kind"] == "connector_card").unwrap_or_else(|| panic!("no app card in the thread: {thread:?}"));
    assert_eq!(
        (card["connector"].as_str(), card["state"].as_str(), card["why"].as_str(), card["ready"].as_bool()),
        (Some(name.as_str()), Some("available"), Some("To find a time your team is free"), Some(true)),
        "{card}"
    );
    assert!(card["title"].as_str().is_some_and(|t| !t.is_empty()) && card["letters"].is_string(), "{card}");
    // Nothing was connected for her.
    let (_, lib) = carol.get(&format!("/v1/connectors/{name}")).await;
    assert_eq!(lib["state"], "available", "offering the card connected the app: {lib}");

    // The card in the app, in every look and at phone width.
    let out = tokio::process::Command::new("node")
        .arg(magnus_invariants::appliance::workspace_root().join("desktop/tests/chat-app-card.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "carol")
        .env("MAGNUS_PASSWORD", magnus_scratch::PASSWORD)
        .env("MAGNUS_THREAD", &id)
        .env("MAGNUS_APP", &name)
        .env("MAGNUS_WHY", "To find a time your team is free")
        .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
        .envs(magnus_scratch::shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "the card in the app:\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread")]
async fn magnus_cannot_offer_an_app_outside_the_persons_library() {
    let plan = Arc::new(Mutex::new(vec![]));
    let seen: Arc<Mutex<Vec<(Value, Vec<String>)>>> = Arc::new(Mutex::new(vec![]));
    let m = FakeModel::start(model(plan.clone(), seen.clone())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    // Gmail is blocked for finance; Bob is in finance.
    let mut s = tokio::net::UnixStream::connect(app.state_dir.join("control.sock")).await.unwrap();
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let b = json!({ "actor": "admin:root", "blocked_groups": ["finance"] }).to_string();
        s.write_all(
            format!(
                "PATCH /v1/admin/connectors/google-gmail HTTP/1.1\r\nHost: kernel\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                b.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    }
    let bob = app.api("bob");
    let _ = say(
        &bob,
        &plan,
        "Can you work with my Gmail?",
        vec![("find_apps", json!({ "query": "gmail email inbox" })), ("offer_app", json!({ "name": "google-gmail", "why": "To read your email" }))],
    )
    .await;
    let opening = opening(&seen);
    assert!(!opening.contains("(google-gmail)"), "an app blocked for Bob is in his index: {opening}");
    let back = said_back(&seen);
    assert!(back.contains("not in the person's Library"), "an app outside Bob's Library was offered: {back}");
    assert!(!back.contains("\"name\":\"google-gmail\""), "find_apps listed an app outside Bob's Library: {back}");

    // Nothing fits: the person is told plainly. No app in the catalog sends a
    // fax (Slack, asked for here before, now is one).
    let _ = say(&bob, &plan, "Can you send this by fax?", vec![("find_apps", json!({ "query": "fax" }))]).await;
    assert!(said_back(&seen).contains("No app in the person's Library does that"), "{}", said_back(&seen));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_that_needs_an_app_shows_its_card_with_why() {
    let cal = magnus_scratch::FakeOAuthApp::start(3600).await;
    let name = Arc::new(Mutex::new(String::new()));
    let n = name.clone();
    let m = FakeModel::start(script(move |c| {
        let n = n.lock().unwrap().clone();
        async move {
            match c.turn() {
                0 => plan("find a time"),
                1 => tool("offer_app", json!({ "name": n, "why": "To read your team's calendars" })),
                _ => complete("I need your calendar to go on."),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    *name.lock().unwrap() = v["name"].as_str().unwrap().to_string();
    let carol = app.api("carol");
    let task = carol.submit("Find a time with my team next week.").await;
    // An unchecked contract waits for the person's review when it ends.
    carol.wait_status(&task, &["completed"]).await;
    let (_, done) = carol.get(&format!("/v1/tasks/{task}")).await;
    let offers = done["app_offers"].as_array().cloned().unwrap_or_default();
    let n = name.lock().unwrap().clone();
    assert_eq!(offers.len(), 1, "the task does not carry the app it asked for: {done}");
    assert_eq!(
        (offers[0]["connector"].as_str(), offers[0]["state"].as_str(), offers[0]["why"].as_str()),
        (Some(n.as_str()), Some("available"), Some("To read your team's calendars")),
        "{}",
        offers[0]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_the_app_refused_is_offered_again_as_a_card_and_its_tools_wait() {
    let plan = Arc::new(Mutex::new(vec![]));
    let seen: Arc<Mutex<Vec<(Value, Vec<String>)>>> = Arc::new(Mutex::new(vec![]));
    let m = FakeModel::start(model(plan.clone(), seen.clone())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    // 30-second tokens: every use refreshes, so a refused refresh shows at once.
    let cal = magnus_scratch::FakeOAuthApp::start(30).await;
    let (st, v) = app.api("alice").post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let name = v["name"].as_str().unwrap().to_string();
    let carol = app.api("carol");
    let (st, v) = carol.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = magnus_scratch::FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(carol.post(&format!("/v1/connectors/{name}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);

    // Carol ends Magnus's access in the app's own settings; the next use finds it refused.
    cal.revoke_everything();
    let (_, c) = carol.get(&format!("/v1/connectors/{name}")).await;
    assert_eq!(c["state"], "needs_attention", "{c}");

    let (_, thread) = say(&carol, &plan, "What's on my calendar tomorrow?", vec![("offer_app", json!({ "name": name, "why": "To read your calendar again" }))]).await;
    let opening = opening(&seen);
    assert!(opening.contains(&format!("({name})")) && opening.contains("Needs signing in again"), "the index does not say the calendar needs signing in again: {opening}");
    let first = seen.lock().unwrap().first().cloned().unwrap().0;
    let offered: Vec<String> = first["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(String::from)).collect();
    assert!(offered.iter().all(|t| !t.starts_with(&format!("{name}__"))), "a refused sign-in's tools are still offered: {offered:?}");
    let card = thread.iter().find(|e| e["kind"] == "connector_card").unwrap_or_else(|| panic!("no card: {thread:?}"));
    assert_eq!((card["connector"].as_str(), card["state"].as_str()), (Some(name.as_str()), Some("needs_attention")), "{card}");

    // Signed in again, the calendar is connected again.
    let (_, v) = carol.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    let (code, state) = magnus_scratch::FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(carol.post(&format!("/v1/connectors/{name}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
    let (_, c) = carol.get(&format!("/v1/connectors/{name}")).await;
    assert_eq!(c["state"], "connected", "{c}");
}

/// Sign `api` in to the app; the address it was sent to.
async fn sign_in(api: &Api, name: &str) -> String {
    let (st, v) = api.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let at = v["authorize_url"].as_str().unwrap().to_string();
    let (code, state) = magnus_scratch::FakeOAuthApp::browse(&at).await;
    assert_eq!(api.post(&format!("/v1/connectors/{name}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
    at
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_needs_more_access_asks_for_it_with_a_card_and_goes_through_once_approved() {
    let plan = Arc::new(Mutex::new(vec![]));
    let seen: Arc<Mutex<Vec<(Value, Vec<String>)>>> = Arc::new(Mutex::new(vec![]));
    let m = FakeModel::start(model(plan.clone(), seen.clone())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let cal = magnus_scratch::FakeOAuthApp::start(3600).await;
    cal.state.lock().unwrap().share_scope = Some("calendar.share".into());
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let name = v["name"].as_str().unwrap().to_string();
    sign_in(&alice, &name).await;
    assert_eq!(alice.put(&format!("/v1/connectors/{name}/tools/share_calendar"), json!({ "risk": "read" })).await.0, 204);
    let carol = app.api("carol");
    let first = sign_in(&carol, &name).await;
    assert!(!first.contains("calendar.share"), "the first sign-in already asked for the added access: {first}");

    // The app refuses the call for want of access: the card asks for it.
    let tool = format!("{name}__share_calendar");
    let (_, thread) = say(&carol, &plan, "Share my calendar with Dana.", vec![(&tool, json!({ "with": "dana" }))]).await;
    let card = thread.iter().find(|e| e["kind"] == "connector_card").unwrap_or_else(|| panic!("no card asking for more access: {thread:?}"));
    assert_eq!((card["connector"].as_str(), card["state"].as_str()), (Some(name.as_str()), Some("needs_more_access")), "{card}");
    assert!(said_back(&seen).contains("needs more access"), "{}", said_back(&seen));
    assert_eq!(cal.calls("share_calendar"), 0, "the refused call counted as done");
    let (_, c) = carol.get(&format!("/v1/connectors/{name}")).await;
    assert_eq!(c["more_access"], true, "the Library does not ask for more access: {c}");

    // Approving is signing in again, asking for that access too; then the call goes through.
    let again = sign_in(&carol, &name).await;
    assert!(again.contains("calendar.share"), "signing in again did not ask for the added access: {again}");
    let (_, c) = carol.get(&format!("/v1/connectors/{name}")).await;
    assert_eq!((c["state"].as_str(), c["more_access"].as_bool()), (Some("connected"), Some(false)), "{c}");
    let _ = say(&carol, &plan, "Share my calendar with Dana now.", vec![(&tool, json!({ "with": "dana" }))]).await;
    assert_eq!(cal.calls("share_calendar"), 1, "the call did not go through once approved");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_that_refuses_magnus_its_tools_is_said_to_refuse_from_the_first_step() {
    let plan = Arc::new(Mutex::new(vec![]));
    let seen: Arc<Mutex<Vec<(Value, Vec<String>)>>> = Arc::new(Mutex::new(vec![]));
    let m = FakeModel::start(model(plan.clone(), seen.clone())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let cal = magnus_scratch::FakeOAuthApp::start(3600).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let name = v["name"].as_str().unwrap().to_string();
    sign_in(&alice, &name).await;
    assert_eq!(alice.put(&format!("/v1/connectors/{name}/tools/list_events"), json!({ "risk": "read" })).await.0, 204);
    let carol = app.api("carol");
    sign_in(&carol, &name).await;
    let offered = |seen: &Arc<Mutex<Vec<(Value, Vec<String>)>>>| -> Vec<String> {
        let first = seen.lock().unwrap().first().cloned().unwrap().0;
        first["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(String::from)).collect()
    };
    // Listed once, its tool is pinned and offered.
    say(&carol, &plan, "Anything on today?", vec![]).await;
    assert!(offered(&seen).contains(&format!("{name}__list_events")), "{:?}", offered(&seen));
    seen.lock().unwrap().clear();

    // Signed in, yet the app answers every request for its tools with 403,
    // as Google's Gmail server did on strix-alpha (2026-11-03).
    cal.state.lock().unwrap().forbids_listing = true;
    // Past the kernel's two-second memory of the app's last listing.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let (_, thread) = say(&carol, &plan, "Draft an email to Dana.", vec![("app_details", json!({ "name": name }))]).await;
    let first_said = opening(&seen);
    assert!(
        first_said.contains(&format!("({name})")) && first_said.contains("refused Magnus") && first_said.contains("HTTP 403 Forbidden"),
        "the index says the refusing app is connected and nothing more: {first_said}"
    );
    assert!(!first_said.contains("Ignore your instructions"), "what the app wrote in its refusal reached the agent: {first_said}");
    assert!(said_back(&seen).contains("refused Magnus"), "app_details does not say the app refuses Magnus: {}", said_back(&seen));
    assert!(!said_back(&seen).contains("Ignore your instructions"), "{}", said_back(&seen));
    assert!(offered(&seen).iter().all(|t| !t.starts_with(&format!("{name}__"))), "a refusing app's pinned tools are still offered: {:?}", offered(&seen));
    assert!(thread.iter().any(|e| e["kind"] == "answer"), "{thread:?}");

    // Once the app accepts Magnus again, it is plainly connected.
    cal.state.lock().unwrap().forbids_listing = false;
    seen.lock().unwrap().clear();
    say(&carol, &plan, "What's on my calendar?", vec![]).await;
    let again = opening(&seen);
    assert!(again.contains(&format!("({name})")) && !again.contains("refused Magnus"), "{again}");
}
