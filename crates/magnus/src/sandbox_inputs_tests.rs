//! A run's sandbox inputs (its attachments and the files it was given) are
//! held in memory only until its workspace takes them: the run's end, an
//! attachment sandbox's end and the person's erasure each let go of them.
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;
use serde_json::json;

fn created(person: &str, task: &str, run: &str) -> Ev {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    serde_json::from_value(json!({ "kind": "task_created", "task": task, "run": run, "person": person, "text": "Read the report.", "config": config })).unwrap()
}

fn files() -> Vec<(String, Vec<u8>)> {
    vec![("report.pdf".into(), b"%PDF-1.7 the report".to_vec())]
}

#[tokio::test(flavor = "multi_thread")]
async fn a_runs_inputs_go_when_it_ends() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", created("mo", "t_ri", "ri")).await.unwrap();
    s.k.sandboxes.set_inputs("ri", files());
    assert!(s.k.sandboxes.holds_inputs("ri"));
    let mo = s.k.person("mo").unwrap();
    s.k.cancel_task(&mo, "t_ri", None).await.unwrap();
    assert!(!s.k.sandboxes.holds_inputs("ri"), "the run's inputs outlived it");
    // An attachment's own sandbox lets go of it once it is done with.
    s.k.sandboxes.set_inputs("attachment-00112233", files());
    s.k.sandboxes.destroy("attachment-00112233").await;
    assert!(!s.k.sandboxes.holds_inputs("attachment-00112233"), "an opened attachment stayed in memory");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erasure_takes_a_persons_inputs() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", created("mo", "t_re", "re")).await.unwrap();
    s.k.emit("person:mo", created("mo", "t_rf", "rf")).await.unwrap();
    s.k.emit("person:sam", created("sam", "t_rs", "rs")).await.unwrap();
    let mo = s.k.person("mo").unwrap();
    s.k.cancel_task(&mo, "t_rf", None).await.unwrap();
    // One going, one ended that was given files after its end, and sam's.
    for run in ["re", "rf", "rs"] {
        s.k.sandboxes.set_inputs(run, files());
    }
    s.k.erase_person("person:sam", "mo").await.unwrap();
    for run in ["re", "rf"] {
        assert!(!s.k.sandboxes.holds_inputs(run), "mo's run {run} kept its inputs after mo was erased");
    }
    assert!(s.k.sandboxes.holds_inputs("rs"), "sam's inputs went with mo");
    s.done().await;
}
