use super::*;
use crate::computer::{KillMode, Ledger};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

/// A computerd that wakes VMs (or is full) and whose guests answer at once.
fn fake_computerd(dir: &Path, full: bool) -> std::path::PathBuf {
    let socket = dir.join("computerd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let reply = match req["op"].as_str().unwrap() {
                        "wake" if full => json!({ "ok": false, "error": "the server runs as many computers as it can" }),
                        "wake" => json!({ "ok": true, "resumed": true, "overlay_fresh": false }),
                        "connect" | "ping" => json!({ "ok": true }),
                        "exec" => json!({ "exit_code": 0 }),
                        "read" => json!({ "data_b64": "SHOT" }),
                        other => json!({ "ok": false, "error": format!("unexpected {other}") }),
                    };
                    w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                }
            });
        }
    });
    socket
}

#[test]
fn any_use_wakes_a_sleeping_computer_but_never_one_paused_by_the_kill_switch() {
    assert_eq!(plan(Status::Running), Plan::Ready);
    for asleep in [Status::Stopped, Status::Hibernated, Status::Killed] {
        assert_eq!(plan(asleep), Plan::Wake, "{asleep:?} needs a start button");
    }
    assert_eq!(plan(Status::Queued), Plan::Waiting);
    assert!(matches!(plan(Status::Paused), Plan::Refused(_)), "a use undid the kill switch");
}

#[test]
fn a_run_uses_its_sleeping_computer_so_its_use_can_wake_it() {
    let mut l = Ledger::default();
    let alice = Owner::Person("alice".into());
    l.apply(&Change::Created { id: "asleep".into(), owner: alice.clone() });
    l.apply(&Change::Started { id: "asleep".into() });
    l.apply(&Change::Hibernated { id: "asleep".into() });
    assert_eq!(pick(l.all(), &alice).as_deref(), Some("asleep"), "a sleeping computer was not used, so nothing wakes it");
    l.apply(&Change::Created { id: "awake".into(), owner: alice.clone() });
    l.apply(&Change::Started { id: "awake".into() });
    assert_eq!(pick(l.all(), &alice).as_deref(), Some("awake"), "a running computer is used first");
    l.apply(&Change::Killed { id: "awake".into(), mode: KillMode::Pause, delete_volume: false });
    l.apply(&Change::Killed { id: "asleep".into(), mode: KillMode::Pause, delete_volume: false });
    assert_eq!(pick(l.all(), &alice), None, "a paused computer was offered to a run");
    assert_eq!(pick(l.all(), &Owner::Person("bob".into())), None);
}

#[test]
fn only_one_use_wakes_a_computer_at_a_time_and_its_progress_shows() {
    assert!(claim("w-one"));
    assert!(!claim("w-one"), "two uses woke one computer");
    assert_eq!(waking("w-one").unwrap()["phase"], "starting");
    set_phase("w-one", Phase::Restoring);
    assert_eq!(waking("w-one").unwrap()["phase"], "restoring");
    {
        let _c = Claimed("w-one");
    }
    assert!(waking("w-one").is_none(), "a finished wake still shows");
    assert!(claim("w-one"));
    release("w-one");
}

#[tokio::test]
async fn a_wake_records_the_start_then_waits_for_the_guest() {
    let d = tempfile::tempdir().unwrap();
    let socket = fake_computerd(d.path(), false);
    assert!(claim("w-boot"));
    let started = AtomicBool::new(false);
    let booted = boot(&socket, "w-boot", || async {
        started.store(true, Ordering::SeqCst);
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(booted, Booted::Woke(ref v) if v["resumed"] == true));
    assert!(started.load(Ordering::SeqCst), "the log never heard it started");
    assert_eq!(waking("w-boot").unwrap()["phase"], "booting");
    release("w-boot");
}

#[tokio::test]
async fn a_full_server_queues_the_wake_instead_of_failing() {
    let d = tempfile::tempdir().unwrap();
    let socket = fake_computerd(d.path(), true);
    let started = AtomicBool::new(false);
    let booted = boot(&socket, "w-full", || async {
        started.store(true, Ordering::SeqCst);
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(booted, Booted::AtCapacity));
    assert!(!started.load(Ordering::SeqCst), "a computer that did not start was recorded as started");
}

#[tokio::test]
async fn the_last_screenshot_is_kept_to_show_while_it_sleeps_and_wakes() {
    let d = tempfile::tempdir().unwrap();
    let socket = fake_computerd(d.path(), false);
    assert!(last_screen("w-shot").is_none());
    assert_eq!(screenshot(&socket, "w-shot").await.unwrap(), "SHOT");
    assert_eq!(last_screen("w-shot").map(|(i, _)| i).as_deref(), Some("SHOT"), "the screenshot before sleep was not kept");
    keep_screen("w-shot", "");
    assert_eq!(last_screen("w-shot").map(|(i, _)| i).as_deref(), Some("SHOT"), "an empty capture replaced the last screenshot");
    forget_screen("w-shot");
    assert!(last_screen("w-shot").is_none(), "a deleted computer's screenshot was kept");
}
