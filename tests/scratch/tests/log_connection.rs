//! Found on strix-alpha (2026-10-29): Postgres dropped every connection
//! ("terminating connection because of crash of another server process"),
//! and the appliance never opened its log again. For forty minutes every
//! write failed while health said ok. Now health says the log is down while
//! it is, and the log opens again, with its single-writer lock, by itself.

use magnus_invariants::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_log_connection_opens_again_and_health_tells_the_truth_meanwhile() {
    // Every task ends at once; what matters is that its end is written.
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let first = alice.submit("Say hello.").await;
    alice.wait_settled(&first).await;

    // Postgres drops the kernel's connections, as when another of its processes crashes.
    let admin = app.db.admin().await;
    let dropped = admin.query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE usename = $1", &[&app.db.kernel_role]).await.unwrap();
    assert!(!dropped.is_empty(), "the kernel had no connection to drop");

    // The log opens again by itself, with its single-writer lock; health is
    // ok only once it can write (terminating is not instant, so wait for both).
    let held = || async {
        admin
            .query_one(
                "SELECT count(*) > 0 FROM pg_locks l JOIN pg_stat_activity a USING (pid) WHERE l.locktype = 'advisory' AND l.objid = 7239001 AND a.usename = $1",
                &[&app.db.kernel_role],
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    };
    let health = eventually("the log open again, with its lock", PATIENCE, || async {
        let (st, v) = alice.get("/v1/health").await;
        (st.as_u16() == 200 && held().await).then_some(v)
    })
    .await;
    assert_eq!(health["log"], "up", "{health}");

    // Work goes on, written to the log.
    let second = alice.submit("Say goodbye.").await;
    let t = alice.wait_settled(&second).await;
    assert_eq!(t["status"], "failed", "{t}");
    let n: i64 = admin.query_one("SELECT count(*) FROM public.events WHERE kind = 'failed'", &[]).await.unwrap().get(0);
    assert_eq!(n, 2, "a task's end is missing from the log");
}

/// Found in CI (2026-10-31, the crash invariant): a kernel started right
/// after one was killed exited with "another kernel holds this event log",
/// because Postgres lets go of a lock only once it sees the old connection
/// closed. A start waits for the log, as a switch of releases does.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_waits_for_the_kernel_before_it_to_let_go_of_the_log() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    app.kill().await;

    // The killed kernel's lock, still held for a moment after it is gone.
    let admin = app.db.admin().await;
    admin.query_one("SELECT pg_advisory_lock(7239001)", &[]).await.unwrap();
    let let_go = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        admin.query_one("SELECT pg_advisory_unlock(7239001)", &[]).await.unwrap();
    });
    app.restart().await;
    let_go.await.unwrap();

    let alice = app.api("alice");
    let t = alice.submit("Say hello.").await;
    assert_eq!(alice.wait_settled(&t).await["status"], "failed");
}

/// CI on pull request 132: this test's restart and another appliance both
/// tried to listen on 127.0.0.1:40059. The restart had written the port it
/// was given into its config and then killed the process, so the port was
/// free for the two seconds this test holds the log's lock. The harness
/// keeps the socket; another bind fails, and the restart still serves there.
#[tokio::test(flavor = "multi_thread")]
async fn a_killed_appliances_port_stays_taken_until_it_restarts() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let port: u16 = app.base.rsplit(':').next().unwrap().parse().unwrap();
    app.kill().await;

    let start = std::time::Instant::now();
    let mut stole = None;
    while start.elapsed() < std::time::Duration::from_millis(500) {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(taken) => {
                stole = Some(taken);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    assert!(stole.is_none(), "127.0.0.1:{port} was free after the appliance was killed, so another test can take it");

    app.restart().await;
    assert!(app.base.ends_with(&format!(":{port}")), "the restart moved off {port} to {}", app.base);
    let alice = app.api("alice");
    let t = alice.submit("Say hello.").await;
    assert_eq!(alice.wait_settled(&t).await["status"], "failed");
}
