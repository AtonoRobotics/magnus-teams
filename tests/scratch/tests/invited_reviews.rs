//! Everyone the server keeps is watched (spec rev 142, Triggers: "Each
//! event wakes the agent"): a person added after setup gets reviews and
//! polls like the config's people. The loop iterated the config's people
//! only, so an invited person was never reviewed while their harness said
//! their email woke Magnus.

use magnus_invariants::appliance::{bin, Appliance, Spec};
use magnus_invariants::*;
use serde_json::json;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";

fn cli(app: &Appliance, args: &[&str], stdin: Option<&str>) {
    let mut c = std::process::Command::new(bin("magnus-appliance"));
    c.args(args).arg("--config").arg(app.dir.path().join("magnus.toml"));
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut c = c.spawn().unwrap_or_else(|e| panic!("magnus {args:?}: {e}"));
    if let (Some(text), Some(mut input)) = (stdin, c.stdin.take()) {
        writeln!(input, "{text}").unwrap_or_else(|e| panic!("magnus {args:?}: {e}"));
    }
    let out = c.wait_with_output().unwrap_or_else(|e| panic!("magnus {args:?}: {e}"));
    assert!(out.status.success(), "magnus {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_added_since_setup_is_reviewed() {
    let reviewed = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = reviewed.clone();
    let m = FakeModel::start(script(move |c| {
        let r = r.clone();
        async move {
            let system = c.body["system"].to_string();
            if system.contains(REVIEW) {
                r.lock().unwrap().push(system);
            }
            tool("task_complete", json!({ "summary": "Reviewed quietly.", "deliverable": [{ "type": "text", "text": "Reviewed quietly." }] }))
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    cli(&app, &["person-add", "--person", "ada", "--name", "Ada Lovelace"], None);
    cli(&app, &["password", "--person", "ada"], Some("ada-long-password"));
    // Signing in refreshes the server's directory, as any sign-in does.
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": "ada", "password": "ada-long-password", "client": "app" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "ada could not sign in: {}", r.status());
    eventually("a review for the person added since setup", Duration::from_secs(90), || async {
        reviewed.lock().unwrap().iter().any(|s| s.contains("You run on this server for Ada Lovelace.")).then_some(())
    })
    .await;
}
