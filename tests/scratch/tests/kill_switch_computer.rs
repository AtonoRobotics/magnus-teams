//! The kill switch reaches the agent's computer (spec rev 141, Admin: Kill
//! switch: "Each stops new work and running work at once, including the
//! agent's computer actions"). At the server's level, a person's and an
//! agent's, a program already running in the computer stops making progress
//! while the switch is on, and goes on from where it was once it is
//! released. Needs a real `magnus-computerd` (KVM, the guest images,
//! passwordless sudo), as on Strix Beta.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::time::Duration;

/// As long as the switch stays on in each round.
const HELD_SECONDS: u64 = 8;
/// The program counts this often, in the computer.
const COUNT_EVERY_MILLIS: u64 = 200;
/// At most this many counts may land around the press and the release; a
/// computer left running would count `HELD_SECONDS` × 5 = 40.
const MOST_WHILE_HELD: u64 = 15;

fn count(computerd: &Computerd, id: &str) -> u64 {
    computerd.exec(id, "cat /tmp/count").trim().parse().unwrap_or(0)
}

async fn status(alice: &Api, id: &str) -> String {
    alice.get(&format!("/v1/computers/{id}")).await.1["status"].as_str().unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kill_switch_stops_the_program_running_in_the_computer_until_its_release() {
    let m = start_checked(script(|_c| async { complete("Done.") })).await;
    let computerd = Computerd::start();
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &m).await;
    let alice = app.api("alice");
    let made: Value = eventually("the server made alice's computer", PATIENCE, || async {
        let (_, v) = alice.get("/v1/computers").await;
        v["computers"].as_array().into_iter().flatten().find(|c| c["owner"]["person"] == "alice").cloned()
    })
    .await;
    let id = made["id"].as_str().unwrap_or_default().to_string();
    let (st, v) = alice.post(&format!("/v1/computers/{id}/start"), json!({})).await;
    assert!(st.is_success(), "start: {v}");
    let every = COUNT_EVERY_MILLIS as f64 / 1000.0;
    computerd.exec(&id, &format!("nohup sh -c 'i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep {every}; done' >/dev/null 2>&1 & echo started"));
    eventually("the program counts", PATIENCE, || async { (count(&computerd, &id) > 2).then_some(()) }).await;
    assert!(set_password(&app, "alice", PASSWORD));

    for (press, release) in [
        ("/v1/admin/kill-switch/press".to_string(), "/v1/admin/kill-switch/release".to_string()),
        ("/v1/admin/kill-switch/person/alice/press".into(), "/v1/admin/kill-switch/person/alice/release".into()),
        ("/v1/admin/kill-switch/agent/person:alice/press".into(), "/v1/admin/kill-switch/agent/person:alice/release".into()),
    ] {
        let before = count(&computerd, &id);
        let (st, v) = alice.post(&press, json!({})).await;
        assert!(st.is_success(), "{press}: {v}");
        assert_eq!(status(&alice, &id).await, "paused", "{press} did not pause the computer");
        tokio::time::sleep(Duration::from_secs(HELD_SECONDS)).await;
        let (st, v) = alice.post("/v1/admin/step-up", json!({ "password": PASSWORD })).await;
        assert!(st.is_success(), "step-up: {v}");
        let (st, v) = alice.post(&release, json!({})).await;
        assert!(st.is_success(), "{release}: {v}");
        assert_eq!(status(&alice, &id).await, "running", "{release} did not resume the computer");
        let after = count(&computerd, &id);
        assert!(after.saturating_sub(before) <= MOST_WHILE_HELD, "the program went on under {press}: {before} then {after}");
        tokio::time::sleep(Duration::from_secs(2)).await;
        let later = count(&computerd, &id);
        assert!(later > after + 3, "the program did not go on after {release}: {after} then {later}");
    }
}
