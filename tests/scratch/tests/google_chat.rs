//! Google Chat, brokered (connectors spec, Brokered API connectors;
//! Google's hosted Chat server refuses Magnus with 403). The catalog's Chat
//! manifest names the image built from this checkout (`brokered/chat`),
//! which runs in each person's own microVM and reaches only
//! `chat.googleapis.com` and Google's identity endpoint, through the appliance's egress proxy. A local
//! HTTPS host stands in for Chat's API, with a certificate for its name
//! from the test's own CA, and FakeOAuthApp for Google's sign-in (the real
//! ones need a Google account: `google_chat_contract.rs` uses the owner's
//! test account).

use magnus_invariants::*;
use magnus_scratch::stand_in::*;
use serde_json::{json, Value};

const HOST: &str = "chat.googleapis.com";
const BASE: &str = "/v1";
const WRITES: [&str; 3] = ["send_message", "update_message", "delete_message"];

async fn chat_signed_in(m: &FakeModel, api: &StandIn) -> (Appliance, magnus_scratch::FakeOAuthApp, magnus_scratch::brokered::Image, StandIn) {
    let identity = StandIn::start("openidconnect.googleapis.com", |r, _| if r.method == "GET" && r.path == "/v1/userinfo" { ok(json!({ "sub": "1" })) } else { not_found() }).await;
    let (app, provider, image) = signed_in_at(m, &[api, &identity], "google-chat", "magnus-chat", "chat").await;
    (app, provider, image, identity)
}

/// What Chat answers, for the requests the tests make: one space, Plans,
/// whose messages come in two pages, the older saying when to ship; a
/// direct conversation with Bob; a message sent comes back as sent.
fn chat(r: &Request, _: &[Request]) -> (u16, String, String) {
    let path = r.path.split('?').next().unwrap_or("");
    let said = |id: &str, who: &str, text: &str| json!({ "name": format!("spaces/AAA/messages/{id}"), "sender": { "name": if who == "Alice" { "users/1" } else { "users/2" }, "type": "HUMAN", "displayName": who }, "text": text, "createTime": "2026-11-03T10:00:00Z", "thread": { "name": "spaces/AAA/threads/T1" } });
    match (r.method.as_str(), path) {
        ("GET", "/v1/spaces") => ok(
            json!({ "spaces": [{ "name": "spaces/AAA", "displayName": "Plans", "spaceType": "SPACE" }, { "name": "spaces/BBB", "displayName": "Lunch", "spaceType": "SPACE" }] }),
        ),
        ("GET", "/v1/spaces/AAA/messages") if r.path.contains("pageToken=p2") => ok(json!({ "messages": [said("m1", "Bob", "We ship on Friday.")] })),
        ("GET", "/v1/spaces/AAA/messages") => ok(json!({ "messages": [said("m3", "Cy", "Lunch?"), said("m2", "Bob", "Who has the keys?")], "nextPageToken": "p2" })),
        ("GET", "/v1/spaces:findDirectMessage") => ok(json!({ "name": "spaces/DM1", "spaceType": "DIRECT_MESSAGE" })),
        ("GET", "/v1/spaces/AAA/members") if r.path.contains("pageToken=members2") => {
            ok(json!({ "memberships": [{ "member": { "name": "users/2", "displayName": "Bob" }, "role": "ROLE_MEMBER" }] }))
        }
        ("GET", "/v1/spaces/AAA/members") => {
            ok(json!({ "memberships": [{ "member": { "name": "users/1", "displayName": "Alice" }, "role": "ROLE_MANAGER" }], "nextPageToken": "members2" }))
        }
        ("GET", "/v1/spaces/AAA/messages/m1") => ok(said("m1", "Alice", "Friday?")),
        ("GET", "/v1/spaces/AAA/messages/m3") => ok(said("m3", "Alice", "Lunch?")),
        ("DELETE", "/v1/spaces/AAA/messages/m3") => (503, String::new(), json!({ "error": { "code": 503, "message": "The service is currently unavailable." } }).to_string()),
        ("GET", "/v1/spaces/AAA/messages/m2") => ok(said("m2", "Bob", "Who has the keys?")),
        ("PATCH", "/v1/spaces/AAA/messages/m1") => {
            let b: Value = serde_json::from_str(&r.body).unwrap();
            ok(said("m1", "Alice", b["text"].as_str().unwrap()))
        }
        ("DELETE", "/v1/spaces/AAA/messages/m1") => ok(json!({})),
        ("DELETE", "/v1/spaces/AAA/messages/m2") => ok(json!({})),
        ("POST", p) if p.ends_with("/messages") => {
            let b: Value = serde_json::from_str(&r.body).unwrap_or_default();
            ok(
                json!({ "name": format!("{}/new", p.trim_start_matches("/v1/")), "text": b["text"], "thread": b["thread"], "sender": { "name": "users/1", "displayName": "Alice" } }),
            )
        }
        _ => not_found(),
    }
}

/// Google permits a space manager to delete other members' messages, but
/// this connector promises only the person's own messages, even if approved.
#[tokio::test(flavor = "multi_thread")]
async fn chat_never_deletes_another_members_message_even_when_approved() {
    let api = StandIn::start(HOST, chat).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("delete a message"),
            1 => tool("chat__delete_message", json!({ "message_id": "spaces/AAA/messages/m2" })),
            _ => complete("done"),
        }
    }))
    .await;
    let (app, _provider, _image, _identity) = chat_signed_in(&m, &api).await;
    let alice = app.api("alice");
    let prompt = "Delete that message.";
    let task = alice.submit(prompt).await;
    let (ask, _) = asked(&alice, &task, &WRITES).await;
    approve(&alice, &ask).await;
    alice.wait_status(&task, &["completed"]).await;
    assert!(api.requests().iter().all(|r| r.method != "DELETE"), "another member's message reached DELETE");
    assert!(told(&m, prompt).contains("only delete messages sent by the signed-in person"));
}

/// Reads expose continuation tokens; editing and deleting each wait for
/// their own approval, even after the other write was approved.
#[tokio::test(flavor = "multi_thread")]
async fn chat_reads_members_across_pages_and_approves_each_edit_and_delete() {
    let api = StandIn::start(HOST, chat).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("review my message, edit it, then remove it"),
            1 => tool("chat__search_conversations", json!({ "query": "Plans" })),
            2 => tool("chat__list_members", json!({ "space_id": "spaces/AAA" })),
            3 => tool("chat__list_members", json!({ "space_id": "spaces/AAA", "page_token": "members2" })),
            4 => tool("chat__get_message", json!({ "message_id": "spaces/AAA/messages/m1" })),
            5 => tool("chat__update_message", json!({ "message_id": "spaces/AAA/messages/m1", "text": "Saturday." })),
            6 => tool("chat__delete_message", json!({ "message_id": "spaces/AAA/messages/m1" })),
            _ => complete("the message is removed"),
        }
    }))
    .await;
    let (app, provider, _image, _identity) = chat_signed_in(&m, &api).await;
    let alice = app.api("alice");
    let prompt = "Review my message, edit it, then remove it.";
    let task = alice.submit(prompt).await;
    let ask = loop {
        let (ask, tool) = asked(&alice, &task, &["list_members", "get_message", "update_message"]).await;
        if tool == "update_message" {
            break ask;
        }
        approve(&alice, &ask).await;
    };
    assert!(api.requests().iter().all(|r| r.method == "GET"));
    assert!(api.requests().iter().any(|r| r.path.contains("pageToken=members2")), "the second membership page was not requested");
    assert!(told(&m, prompt).contains("members2"), "the first membership page hid its continuation token");
    approve(&alice, &ask).await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert_eq!(tool, "delete_message");
    assert_eq!(api.requests().iter().filter(|r| r.method == "PATCH").count(), 1);
    assert!(api.requests().iter().all(|r| r.method != "DELETE"));
    approve(&alice, &ask).await;
    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    let calls = api.requests();
    let edit = calls.iter().find(|r| r.method == "PATCH").unwrap();
    assert_eq!(edit.path, "/v1/spaces/AAA/messages/m1?updateMask=text");
    assert_eq!(serde_json::from_str::<Value>(&edit.body).unwrap(), json!({ "text": "Saturday." }));
    assert_eq!(calls.iter().filter(|r| r.method == "DELETE").count(), 1);
    only_the_api(&app, &api, &provider, BASE);
}

/// A message is seen by everyone in the conversation, and cannot be
/// unseen, so sending one always asks first: here Magnus has read nothing
/// yet, and nothing reaches Chat until Alice approves it.
#[tokio::test(flavor = "multi_thread")]
async fn chat_sends_a_message_only_once_the_person_approves_it_reaching_only_google() {
    let api = StandIn::start(HOST, chat).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("tell the Plans space I'll bring the keys"),
            1 => tool("chat__send_message", json!({ "space_id": "spaces/AAA", "text": "I'll bring the keys.", "thread_id": "spaces/AAA/threads/T1" })),
            _ => complete("they know"),
        }
    }))
    .await;
    let (app, provider, _image, _identity) = chat_signed_in(&m, &api).await;
    let alice = app.api("alice");

    let task = alice.submit("Tell the Plans space I'll bring the keys.").await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert_eq!(tool, "send_message");
    assert!(api.requests().is_empty(), "Chat was called before Alice approved: {:?}", api.calls(BASE));
    approve(&alice, &ask).await;

    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    let sent = &api.requests()[0];
    assert_eq!((api.requests().len(), sent.method.as_str(), sent.path.as_str()), (1, "POST", "/v1/spaces/AAA/messages?messageReplyOption=REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD"));
    assert_eq!(serde_json::from_str::<Value>(&sent.body).unwrap(), json!({ "text": "I'll bring the keys.", "thread": { "name": "spaces/AAA/threads/T1" } }));
    only_the_api(&app, &api, &provider, BASE);
}

/// A search reads the space's messages page after page for the words, and
/// having read them, Magnus asks before it sends anything (the Rule of Two
/// and the send's own class); a message to a person goes to the
/// conversation with them.
#[tokio::test(flavor = "multi_thread")]
async fn chat_finds_words_across_pages_and_asks_before_telling_someone() {
    let api = StandIn::start(HOST, chat).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("find when we ship and tell Dee"),
            1 => tool("chat__search_messages", json!({ "space_id": "spaces/AAA", "query": "ship" })),
            2 => tool("chat__send_message", json!({ "to_email": "dee@example.com", "text": "We ship on Friday." })),
            _ => complete("Dee knows"),
        }
    }))
    .await;
    let (app, provider, _image, _identity) = chat_signed_in(&m, &api).await;
    let alice = app.api("alice");
    let get = |p: &str| ("GET".to_string(), p.to_string());

    let task = alice.submit("Find when we ship and tell Dee.").await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert_eq!((tool.as_str(), api.calls(BASE)), ("send_message", vec![get("/spaces/AAA/messages"), get("/spaces/AAA/messages")]));
    assert!(api.requests()[1].path.contains("pageToken=p2"), "{:?}", api.requests()[1]);
    let said = told(&m, "Find when we ship and tell Dee.");
    assert!(said.contains("We ship on Friday.") && !said.contains("Who has the keys?"), "the search's finding did not reach Magnus alone: {said}");
    approve(&alice, &ask).await;

    alice.wait_status(&task, &["completed"]).await;
    assert_eq!(alice.wait_settled(&task).await["status"], "completed");
    let r = api.requests();
    assert_eq!(r[2].path, "/v1/spaces:findDirectMessage?name=users%2Fdee%40example.com");
    assert_eq!((r[3].method.as_str(), r[3].path.as_str()), ("POST", "/v1/spaces/DM1/messages"));
    only_the_api(&app, &api, &provider, BASE);
}

/// Google's refusals reach the boundary as what they mean: a sign-in
/// without Chat's access asks the person for it with the app's card, and a
/// delete with no clear answer is an unknown outcome.
#[tokio::test(flavor = "multi_thread")]
async fn chat_s_refusals_reach_the_boundary_as_what_they_mean() {
    let api = StandIn::start(HOST, chat).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("delete my lunch message in Plans"),
            1 => tool("chat__list_messages", json!({ "space_id": "spaces/AAA" })),
            2 => tool("chat__delete_message", json!({ "message_id": "spaces/AAA/messages/m3" })),
            _ => complete("done"),
        }
    }))
    .await;
    let (app, provider, _image, _identity) = chat_signed_in(&m, &api).await;
    let alice = app.api("alice");
    let scope = "https://www.googleapis.com/auth/chat.messages";
    api.then(
        403,
        &format!("WWW-Authenticate: Bearer realm=\"https://accounts.google.com/\", error=\"insufficient_scope\", scope=\"{scope}\"\r\n"),
        json!({ "error": { "code": 403, "message": "Request had insufficient authentication scopes.", "status": "PERMISSION_DENIED", "details": [{ "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] } }),
    );
    let task = alice.submit("Delete my lunch message in Plans.").await;
    let (ask, tool) = asked(&alice, &task, &WRITES).await;
    assert!(told(&m, "Delete my lunch message in Plans.").contains("needs more access from the person for list_messages"), "the refusal was not a request for access");
    assert_eq!(tool, "delete_message");
    approve(&alice, &ask).await;
    eventually("the delete's unknown outcome", std::time::Duration::from_secs(60), || async {
        app.logs().contains("delete_message: outcome unknown: Google Chat answered HTTP 503 and whether it did this is unknown.").then_some(())
    })
    .await;
    only_the_api(&app, &api, &provider, BASE);
}
