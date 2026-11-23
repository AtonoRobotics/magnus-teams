//! An update either finishes or is undone, through the store's faults:
//! a trigger makes the log refuse the writes named.

use crate::events::Ev;
use crate::state::Status;
use serde_json::{json, Value};

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

/// The log refuses every event of `kind` once `after` of them are in it.
async fn refuse(db: &tokio_postgres::Client, kind: &str, after: i64) {
    db.batch_execute(&format!(
        "CREATE OR REPLACE FUNCTION refuse_kind() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
         BEGIN
           IF NEW.kind = '{kind}' AND (SELECT count(*) FROM public.events WHERE kind = '{kind}') >= {after} THEN
             RAISE EXCEPTION 'the store refuses {kind}';
           END IF;
           RETURN NEW;
         END $$;
         CREATE TRIGGER refuse_kind BEFORE INSERT ON public.events FOR EACH ROW EXECUTE FUNCTION refuse_kind();"
    ))
    .await
    .unwrap();
}

async fn mend(db: &tokio_postgres::Client) {
    db.batch_execute("DROP TRIGGER refuse_kind ON public.events").await.unwrap();
}

/// A store that fails the second suspend: the switch is undone, with no
/// task left suspended for it, the server no longer updating, and the
/// failure recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_whose_second_suspend_fails_leaves_no_task_suspended_for_it() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let k = &s.k;
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    for (task, run) in [("t1", "r1"), ("t2", "r2"), ("t3", "r3")] {
        k.emit("person:mo", ev(json!({ "kind": "task_created", "task": task, "run": run, "person": "mo", "text": "x", "config": config }))).await.unwrap();
    }
    let db = s.store().await;
    refuse(&db, "suspended", 1).await;
    // The programs are never put in place: nothing here may replace the test.
    let out = k.switch("person:sam", "1.0.0", "1.0.1", false, |_| Err(anyhow::anyhow!("no programs in a test"))).await.unwrap();
    mend(&db).await;
    assert_eq!(out["status"], "failed", "{out}");
    let suspended: Vec<String> = k.read(|s| s.runs.values().filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("update")).map(|r| r.id.clone()).collect());
    assert!(suspended.is_empty(), "left suspended for an update that failed: {suspended:?}");
    for run in ["r1", "r2", "r3"] {
        assert_eq!(k.read(|s| s.runs[run].status), Status::Running, "{run}");
    }
    assert!(k.read(|s| s.server_ops.updating().is_none()), "the server is still updating");
    assert_eq!(k.read(|s| s.server_ops.last_update().map(|u| u.what.clone())), Some("failed".into()));
    assert!(!crate::update::switching(&k.cfg), "the pending switch is still written down");
    s.done().await;
}

/// A release that started and served, whose finish the store would not
/// record, is not rolled back at the next start; that start records it.
#[tokio::test(flavor = "multi_thread")]
async fn a_finish_the_store_did_not_record_does_not_roll_back_the_next_start() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let k = &s.k;
    k.emit("person:sam", Ev::UpdateStarted { from: "1.0.0".into(), to: "1.0.1".into(), rollback: false }).await.unwrap();
    // As the switch leaves it, and the new release's first start marks it.
    super::write_pending(&k.cfg, &json!({ "from": "1.0.0", "to": "1.0.1", "rollback": false, "owner": false, "reason": "", "started_as": { "program": "magnus", "args": [] } }))
        .unwrap();
    super::on_start(&k.cfg).unwrap();
    let db = s.store().await;
    refuse(&db, "update_finished", 0).await;
    assert!(k.after_start().await.is_err(), "the finish was recorded in a store that refuses it");
    mend(&db).await;
    // The next start: not taken for a release that did not come up.
    super::on_start(&k.cfg).expect("the next start rolled back a release that came up");
    k.after_start().await.unwrap();
    assert!(!crate::update::switching(&k.cfg), "the pending switch outlived its record");
    assert_eq!(k.read(|s| s.server_ops.last_update().map(|u| u.what.clone())), Some("updated".into()));
    s.done().await;
}
