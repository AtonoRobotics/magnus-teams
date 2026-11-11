//! Scenario 8: the agent uses its computer with nobody having set one up
//! (spec rev 142, "One computer, always there"). Part of the autonomy suite,
//! kept in its own file for size: `cargo test -p magnus-scratch --test
//! autonomy`. Needs a real `magnus-computerd` (KVM, the guest images,
//! passwordless sudo), as on Strix Beta.

use super::*;

/// 8. The person never makes, picks or starts a computer. The server made
/// hers when it started; it has never run. She asks in chat what is on her
/// computer's screen: the agent's first desktop tool wakes that computer and
/// works, with no "You have no running computer", no approval, and still
/// exactly one computer, now awake.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_uses_its_computer_with_nobody_having_set_one_up() {
    const ASK: &str = "Look at my computer's screen and tell me what is open.";
    const ANSWER: &str = "Your computer's desktop is open, with nothing else running.";
    let m = FakeModel::start(script(|c| async move {
        if run_of(&c) != Run::Chat || !said(&c).contains(ASK) {
            return background(&c);
        }
        match c.turn() {
            0 => tool("computer_screenshot", json!({})),
            _ => say(ANSWER),
        }
    }))
    .await;
    let computerd = Computerd::start();
    let app = Appliance::start(proactive_on().computer(&computerd.socket), &m).await;
    let alice = app.api("alice");

    // Her one computer is there, made by the server, never started.
    let mine = || async {
        let (st, v) = alice.get("/v1/computers").await;
        assert!(st.is_success(), "{v}");
        let own: Vec<Value> = v["computers"].as_array().into_iter().flatten().filter(|c| c["owner"]["person"] == "alice").cloned().collect();
        own
    };
    let made = eventually("the server made alice's computer", PATIENCE, || async {
        let own = mine().await;
        (own.len() == 1).then(|| own[0].clone())
    })
    .await;
    let id = made["id"].as_str().unwrap_or_default().to_string();
    assert_ne!(made["status"], "running", "the computer was running before anything used it: {made}");
    assert!(computerd.procs(&id).is_empty(), "the computer was started before anything used it");
    let b = before(&alice).await;

    let (thread, turn) = chat(&alice, ASK).await;
    let t = alice.wait_settled(&turn).await;
    assert_eq!(t["status"], "completed", "{t}\n-- the model saw:\n{}\n-- the inbox: {:?}", story(&m), alice.inbox().await);

    // The desktop tool worked on her computer, woken for it.
    let r = m.requests().into_iter().find(|c| run_of(c) == Run::Chat && said(c).contains(ASK) && c.turn() == 1).expect("the turn went no further").last_results();
    assert!(r.len() == 1 && !r[0].is_error, "the screenshot was refused or failed: {r:?}\n{}", story(&m));
    assert!(r[0].text.contains("Screenshot of your computer"), "the screenshot was not of her computer: {r:?}");
    assert!(!story(&m).contains("no running computer") && !story(&m).contains("have no computer"), "the agent was told it has no computer:\n{}", story(&m));
    let now = mine().await;
    assert_eq!(now.len(), 1, "she has {} computers", now.len());
    assert_eq!((now[0]["id"].as_str(), now[0]["status"].as_str()), (Some(id.as_str()), Some("running")), "the use did not wake her one computer: {:?}", now[0]);
    assert!(!computerd.procs(&id).is_empty(), "no VM runs for her computer");

    // She sees her message and the answer; nothing else reached her.
    let e = entries(&alice, &thread).await;
    assert_eq!(kinds(&e), ["message", "answer"], "{e:?}");
    assert_eq!(e[1]["text"], ANSWER);
    only(&alice, &b, &[], &[], &[&thread]).await;
}
