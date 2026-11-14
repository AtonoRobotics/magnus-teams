//! The same app card in approvals (connectors spec, The connector card:
//! "The same card appears in the Library, in chat, in a task, and in
//! approvals"). An approval for an app's action carries the app's card as
//! the person whose task it is stands with it: connected while it is, and
//! asking them to sign in again once the app has refused their sign-in.
//! Whoever decides it sees where they stand, and only they sign in. In the
//! app, the card is in the decision in Needs you, in every look and at
//! phone width.

use magnus_invariants::*;
use magnus_scratch::FakeOAuthApp;
use serde_json::{json, Value};

async fn approval(api: &Api, task: &str) -> Value {
    api.inbox().await.into_iter().find(|i| i["task_id"] == task && i["kind"] == "approval" && i["status"] == "open").expect("no open approval")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approval_for_an_apps_action_carries_the_apps_card_as_the_person_stands_with_it() {
    // 30-second tokens: every use refreshes, so a refused sign-in shows at once.
    let cal = FakeOAuthApp::start(30).await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("list the events"),
            1 => tool("calendar__list_events", json!({})),
            _ => complete("listed"),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "public", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    // Every use of list_events asks first.
    assert_eq!(alice.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "irreversible" })).await.0, 204);
    let carol = app.api("carol");
    assert!(magnus_scratch::set_password(&app, "carol", magnus_scratch::PASSWORD));
    let (st, v) = carol.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(carol.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);

    let task = carol.submit("List my events.").await;
    carol.wait_item(&task, "approval").await;
    let item = approval(&carol, &task).await;
    let card = &item["app"];
    assert_eq!((card["kind"].as_str(), card["connector"].as_str(), card["state"].as_str()), (Some("connector_card"), Some("calendar"), Some("connected")), "{item}");
    assert!(card["title"].as_str().is_some_and(|t| !t.is_empty()) && card["letters"].is_string(), "{card}");

    // Carol ends Magnus's access in the app's own settings; the card in the
    // approval now asks her to sign in again.
    cal.revoke_everything();
    let (_, c) = carol.get("/v1/connectors/calendar").await;
    assert_eq!(c["state"], "needs_attention", "{c}");
    let item = approval(&carol, &task).await;
    assert_eq!(item["app"]["state"], "needs_attention", "{item}");

    // In the app: Carol, whose app it is, signs in again from the decision;
    // Alice, who decides it, sees where Carol stands and no button of hers.
    assert!(magnus_scratch::set_password(&app, "alice", magnus_scratch::PASSWORD));
    let decider = approval(&alice, &task).await;
    assert_eq!((decider["can_respond"].as_bool(), decider["app"]["state"].as_str()), (Some(true), Some("needs_attention")), "{decider}");
    for (who, decides) in [("carol", false), ("alice", true)] {
        let mut run = tokio::process::Command::new("node");
        run.arg(magnus_invariants::appliance::workspace_root().join("desktop/tests/approval-app-card.mjs"))
            .env("MAGNUS_URL", &app.base)
            .env("MAGNUS_PERSON", who)
            .env("MAGNUS_PASSWORD", magnus_scratch::PASSWORD)
            .env("MAGNUS_ITEM", item["id"].as_str().unwrap())
            .env("MAGNUS_APP", "calendar")
            .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
            .envs(magnus_scratch::shots());
        if decides {
            run.env("MAGNUS_DECIDES", "1");
        }
        let out = run.output().await.unwrap();
        assert!(out.status.success(), "the card in the approval, for {who}:\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
}
