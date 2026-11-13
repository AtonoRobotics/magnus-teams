//! An update leaves the person's inbox holding only the four asks of spec
//! rev 140: items left open under retired asks (a criteria review, a
//! contract change, an approval outside the four) are put in their current
//! form at startup, and the runs waiting on them go on
//! (decisions/2026-11-10-permissions-asks.md).

use magnus_invariants::*;
use serde_json::{json, Value};

const WITHDRAWN: &str = "the approval it waited for is no longer asked";
const FROZEN: &str = "Task contracts are retired";

/// A run left waiting on an item an earlier release raised: each task asks
/// a question, and the test swaps the question for the retired item, as the
/// earlier release's log held it.
async fn waiting_on(alice: &Api, text: &str) -> (String, String, String) {
    let t = alice.submit(text).await;
    let q = alice.wait_item(&t, "question").await;
    let run = alice.task(&t).await["run_id"].as_str().unwrap_or_default().to_string();
    let call = q["details"]["call_id"].as_str().unwrap_or_default().to_string();
    assert!(!call.is_empty(), "the question has no call: {q}");
    (t, run, call)
}

async fn insert(app: &Appliance, run: Option<&str>, body: Value) {
    let kind = body["kind"].as_str().unwrap_or_default().to_string();
    let r =
        app.db.admin().await.execute("INSERT INTO public.events (kind, actor, run_id, body) VALUES ($1, 'kernel', $2, $3)", &[&kind, &run, &body]).await.map_err(|e| e.to_string());
    assert!(r.is_ok(), "{r:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn items_of_retired_asks_are_put_in_their_current_form_at_startup() {
    let m = FakeModel::start(script(|c| async move {
        let told: String = c.last_results().iter().map(|r| r.text.clone()).collect();
        if told.contains(WITHDRAWN) || told.contains(FROZEN) {
            return tool("task_fail", json!({ "reason": "went on after the update" }));
        }
        tool("ask_principal", json!({ "question": "Ready?" }))
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let review = waiting_on(&alice, "Write me a poem.").await;
    let change = waiting_on(&alice, "Close the record.").await;
    let approval = waiting_on(&alice, "Post the summary.").await;
    let questions: Vec<String> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "question").map(|i| i["id"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(questions.len(), 3);

    app.kill().await;
    for q in &questions {
        insert(&app, None, json!({ "kind": "item_resolved", "id": q, "decision": "dismiss" })).await;
    }
    let item = |id: &str, kind: &str, (task, run, call): &(String, String, String), details: Value| {
        let mut d = details;
        d["call_id"] = json!(call);
        (run.clone(), json!({ "kind": "item", "id": id, "item_kind": kind, "task": task, "run": run, "person": "alice", "summary": "From an earlier release", "details": d }))
    };
    for (run, body) in [
        item("item_old_review", "review", &review, json!({ "criteria": [{ "id": "c1", "description": "the poem is moving" }], "summary": "Wrote the poem." })),
        item("item_old_change", "contract_change", &change, json!({ "criteria": [] })),
        item("item_old_approval", "approval", &approval, json!({ "tool": "chat__post", "rule": "kernel:audience", "risk": "irreversible" })),
    ] {
        insert(&app, Some(&run), body).await;
    }
    app.restart().await;

    // Nothing of a retired kind waits on the person.
    // (A run that went on and failed leaves its notice; that is not an ask.)
    let open: Vec<Value> = alice.inbox().await.into_iter().filter(|i| i["status"] == "open" && i["kind"] != "notice").collect();
    assert!(open.is_empty(), "items of retired asks are still open after the update: {open:#?}");
    // The review's run completed, its criterion unconfirmed.
    let t = alice.wait_settled(&review.0).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(t["summary"], "Wrote the poem.", "{t}");
    assert!(app.db.dump().await.contains("Not confirmed by any check: c1"), "the criterion was not recorded as unconfirmed");
    // The others went on, told why.
    for (task, said) in [(&change.0, FROZEN), (&approval.0, WITHDRAWN)] {
        let t = alice.wait_settled(task).await;
        assert_eq!(t["status"], "failed", "{t}");
        assert!(m.requests().iter().any(|c| c.last_results().iter().any(|r| r.text.contains(said))), "the run was not told: {said}");
    }
}
