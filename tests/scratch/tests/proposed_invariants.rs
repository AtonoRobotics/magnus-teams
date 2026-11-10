//! Tightened invariant tests, proposed for `tests/invariants/` (see
//! `docs/invariant-mutations.md` for the mutants each one was written to
//! kill). They live here until the owner applies
//! `docs/proposals/invariant-tests-tightened.patch`.

use magnus_invariants::{mcp, *};
use magnus_scratch::start_checked;
use serde_json::json;
use std::{collections::BTreeMap, time::Duration};

// ---- inv1_boundary.rs

#[tokio::test(flavor = "multi_thread")]
async fn action_is_not_executed_when_its_intent_cannot_be_recorded() {
    let mail = mcp::mail().start().await;
    let model = start_checked(steps(vec![
        contract_unchecked("send a note", "note sent"),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("stopping"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    // The log refuses exactly this action's intent; everything before it,
    // the model's reply and the boundary's decision, is recorded.
    app.db
        .admin()
        .await
        .batch_execute(
            "CREATE FUNCTION public.refuse_intent() RETURNS trigger LANGUAGE plpgsql AS
               $$ BEGIN RAISE EXCEPTION 'the log refuses this intent'; END $$;
             CREATE TRIGGER refuse_send_intent BEFORE INSERT ON public.events FOR EACH ROW
               WHEN (NEW.kind = 'intent' AND NEW.body->>'tool' = 'mail__send')
               EXECUTE FUNCTION public.refuse_intent();",
        )
        .await
        .expect("install the trigger");
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());

    alice.submit("Email bob a hello.").await;
    let db = app.db.admin_url();
    eventually("the boundary allowed the send", PATIENCE, || {
        let db = db.clone();
        async move {
            let c = magnus_invariants::pg::connect(&db).await;
            let n: i64 = c
                .query_one("SELECT count(*) FROM public.events WHERE kind = 'decision' AND body->>'tool' = 'mail__send' AND body->>'outcome' = 'allow'", &[])
                .await
                .unwrap()
                .get(0);
            (n > 0).then_some(())
        }
    })
    .await;
    never("the send executed without a recorded intent", Duration::from_secs(10), || async { !mail.calls_to("send").is_empty() }).await;
}

// ---- inv3_event_log.rs

/// Every crash lands while a post is in flight: its effect has happened and
/// its response is lost. The resend after each restart must be recognized.
#[tokio::test(flavor = "multi_thread")]
async fn repeated_crashes_never_repeat_an_effect() {
    let chat = mcp::chat().start().await;
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_checked("post four updates", "chat__list_posts", json!({}), "/count", json!(4)),
            n @ 1..=4 => tool("chat__post", json!({ "channel": "team", "text": format!("update {n}") })),
            _ => complete("posted"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("chat", &chat, "internal", "private", &[("post", "irreversible"), ("list_posts", "read")]), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("chat__post"))).await.is_success());

    // A held call is one that took effect; the app answers a recognized
    // resend without holding it, so each hold catches the next new post.
    let mut hold = chat.hold("post");
    let id = alice.submit("Post four updates to the team channel.").await;
    for n in 1..=4 {
        tokio::time::timeout(PATIENCE, hold.received()).await.unwrap_or_else(|_| panic!("post {n} never reached the app"));
        app.kill().await;
        let next = (n < 4).then(|| chat.hold("post"));
        hold.release();
        app.restart().await;
        if let Some(h) = next {
            hold = h;
        }
    }
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(app.starts(), 5);
    let mut keys: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for c in chat.calls_to("post") {
        keys.entry(c.args["text"].as_str().unwrap().to_string()).or_default().push(c.idempotency_key().map(String::from));
    }
    for n in 1..=4 {
        let text = format!("update {n}");
        let k = chat.world()["posts"].as_array().unwrap().iter().filter(|p| p["text"] == text.as_str()).count();
        assert_eq!(k, 1, "{text} was posted {k} times across four crashes");
        let sent = &keys[&text];
        assert!(sent.len() >= 2, "{text} was not resent after its crash: {sent:?}");
        assert!(sent.iter().all(|k| k.is_some() && *k == sent[0]), "the resends of {text} did not carry one idempotency key: {sent:?}");
    }
}

// ---- inv6_policy.rs

fn send(body: &str) -> Reply {
    tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": body }))
}

/// The second send is identical to the first, so only the approval having
/// been used stops it.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_action_approval_covers_one_action() {
    let model =
        start_checked(steps(vec![contract_unchecked("say hi twice", "said"), send(magnus_scratch::OUTSIDE_THE_ASK), send(magnus_scratch::OUTSIDE_THE_ASK), fail("done")])).await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
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

// ---- inv7_verify.rs

/// Checking is internal (spec rev 140): a criterion no check can confirm is
/// never put to the person; the task completes and the criterion is recorded
/// as unconfirmed.
#[tokio::test(flavor = "multi_thread")]
async fn a_criterion_without_a_check_is_never_put_to_the_person() {
    let model = start_checked(steps(vec![contract_unchecked("write a poem", "the poem is moving"), complete("wrote it"), fail("unexpected turn")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");

    let id = alice.submit("Write me a poem.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert!(alice.inbox().await.iter().all(|i| i["kind"] != "review"), "the person was asked to review criteria");
    assert!(app.db.dump().await.contains("Not confirmed by any check: c1"), "the unconfirmed criterion is not recorded");
}
