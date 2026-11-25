//! "Not now" keeps a card (owner, 2026-11-23): put off, it is accepted or
//! declined later; accepted or declined, it is decided.
use super::CardRefusal;
use crate::state::Answer;
use serde_json::json;

fn card_result(call_id: &str, card: &str) -> crate::events::Ev {
    crate::events::Ev::ToolResult {
        run: "r_turn".into(),
        call_id: call_id.into(),
        key: None,
        status: "done".into(),
        text: "A suggestion.".into(),
        is_error: false,
        label: crate::tools::Label::default(),
        // Timed, so accepting it sets a schedule.
        data: Some(json!({ "card": card, "suggestion": true, "title": "Tidy the inbox", "text": "Archive last week's newsletters.", "every_minutes": 1440 })),
        sources: vec![],
        check: false,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_card_put_off_is_answered_later_and_one_decided_is_not() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let k = &s.k;
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    let turn = json!({ "kind": "task_created", "task": "t_turn", "run": "r_turn", "person": "mo", "config": config, "chat": { "thread": "th_mo", "turn": true } });
    k.emit("person:mo", serde_json::from_value(turn).unwrap()).await.unwrap();
    k.emit("kernel", card_result("c1", "card_a")).await.unwrap();
    k.emit("kernel", card_result("c2", "card_b")).await.unwrap();
    let answer = |card: &str| k.read(|st| st.arrangements.cards().get(card).map(|c| c.answer.clone()));
    let decide = |card: &'static str, decision: &'static str, reason: Option<&'static str>| async move {
        k.decide_card("mo", card, decision, reason, &json!({})).await.unwrap_or_else(|e| panic!("{card} {decision}: {e:#}"))
    };

    // Put off, it is kept, and putting it off again changes nothing.
    assert!(decide("card_a", "later", None).await.is_ok(), "Not now was refused");
    assert!(matches!(answer("card_a"), Some(Answer::Later)));
    assert!(matches!(decide("card_a", "later", None).await, Err(CardRefusal::Decided)), "a card was put off twice");
    // Later, it is accepted: its schedule is set.
    let Ok(set) = decide("card_a", "accept", None).await else { panic!("a card put off could not be accepted") };
    assert!(set["schedule"].is_string(), "{set}");
    assert!(matches!(answer("card_a"), Some(Answer::Confirmed(_))));
    // Accepted, it is decided.
    assert!(matches!(decide("card_a", "decline", Some("changed my mind")).await, Err(CardRefusal::Decided)), "an accepted card was declined");

    // Put off, then declined: removed as a decline is, and decided after.
    assert!(decide("card_b", "later", None).await.is_ok());
    assert!(decide("card_b", "decline", Some("not needed")).await.is_ok(), "a card put off could not be declined");
    assert!(matches!(answer("card_b"), Some(Answer::Declined(_))));
    assert!(matches!(decide("card_b", "accept", None).await, Err(CardRefusal::Decided)), "a declined card was accepted");
    s.done().await;
}
