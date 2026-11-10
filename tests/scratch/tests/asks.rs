//! An ask end to end (spec rev 140, How an ask looks): the person sees the
//! server's card, never the reasoning, and the agent never sees the card;
//! Allow covers the rest of the task for the same kind and destinations,
//! Allow always saves a narrow rule that answers next time, and Don't allow
//! stops that action while the agent carries on without it.

use magnus_invariants::{mcp, *};
use magnus_scratch::{start_checked, OUTSIDE_THE_ASK};
use serde_json::{json, Value};

/// A send outside what was asked (case 1, through the Auto Review player),
/// to carol when the task names her and to bob otherwise, `sends` times.
async fn world(sends: usize) -> (FakeMcp, FakeModel, Appliance) {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(move |c| async move {
        let to = if c.messages()[0].to_string().contains("carol") { "carol@corp.example" } else { "bob@corp.example" };
        let turn = c.turn();
        if turn == 0 {
            return contract_unchecked("send the note", "the note was handled");
        }
        if c.text().contains("did not allow") {
            return complete("the person did not want the note sent, so it was not");
        }
        if turn <= sends {
            return tool("mail__send", json!({ "to": to, "subject": "note", "body": OUTSIDE_THE_ASK }));
        }
        complete("handled")
    }))
    .await;
    let s = Spec::default().app("mail", &mail, "public", "private", &[("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    (mail, m, app)
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_person_sees_one_sentence_and_the_agent_never_sees_it() {
    let (mail, m, app) = world(1).await;
    let alice = app.api("alice");
    let t = alice.submit("Send bob the note.").await;
    let card = alice.wait_item(&t, "approval").await;
    let sentence = card["ask"]["sentence"].as_str().unwrap_or_default().to_string();
    assert!(sentence.starts_with("Magnus wants to") && sentence.contains("bob@corp.example"), "{card}");
    assert_eq!(card["ask"]["offers"], json!(["allow", "allow_always", "dont_allow"]), "{card}");
    assert_eq!(card["ask"]["allow_scope"], "task", "{card}");
    for hidden in ["why", "rule", "taint", "reason"] {
        assert!(card["details"][hidden].is_null(), "the card carries {hidden}: {card}");
    }
    assert_eq!(card["details"]["input"]["body"], OUTSIDE_THE_ASK, "the exact content is not one tap away: {card}");
    // Don't allow: that action stops and the agent carries on without it.
    assert!(alice.respond(id(&card), json!({ "decision": "reject" })).await.is_success());
    let done = alice.wait_settled(&t).await;
    assert_eq!(done["status"], "completed", "the agent did not carry on: {done}");
    assert_eq!(mail.effects("send"), 0, "a send nobody allowed went out");
    let seen: String = m.requests().iter().map(|c| c.text()).collect();
    assert!(!seen.contains(&sentence), "the agent saw the card");
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_covers_the_rest_of_the_task_for_the_same_destination_only() {
    let (mail, _m, app) = world(2).await;
    let alice = app.api("alice");
    let t = alice.submit("Send bob the note twice.").await;
    let card = alice.wait_item(&t, "approval").await;
    assert!(alice.respond(id(&card), json!({ "decision": "approve", "scope": "task" })).await.is_success());
    let done = alice.wait_settled(&t).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(mail.effects("send"), 2, "Allow did not cover the second send of the same kind");
    let asks = alice.inbox().await.into_iter().filter(|i| i["kind"] == "approval" && i["task_id"] == t.as_str()).count();
    assert_eq!(asks, 1, "the second send asked again");
    // Another task asks again: Allow lasts for its task only.
    let t2 = alice.submit("Send bob the note again.").await;
    alice.wait_item(&t2, "approval").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_always_saves_a_narrow_rule_that_answers_next_time() {
    let (mail, _m, app) = world(1).await;
    let alice = app.api("alice");
    let t = alice.submit("Send bob the note.").await;
    let card = alice.wait_item(&t, "approval").await;
    // Only the person whose work it is may say it.
    assert!(!app.api("bob").respond(id(&card), json!({ "decision": "approve", "scope": "always" })).await.is_success(), "someone else saved a rule for alice");
    assert!(alice.respond(id(&card), json!({ "decision": "approve", "scope": "always" })).await.is_success());
    alice.wait_settled(&t).await;
    assert_eq!(mail.effects("send"), 1);
    // The same send to the same person in a new task goes without asking.
    let t = alice.submit("Send bob the note again.").await;
    let done = alice.wait_settled(&t).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(mail.effects("send"), 2, "the rule did not answer the same send");
    assert!(alice.open_items(&t, "approval").await.is_empty());
    // A send to someone else still asks: the rule is narrow.
    let t = alice.submit("Send carol the note.").await;
    alice.wait_item(&t, "approval").await;
    assert_eq!(mail.effects("send"), 2);
}
