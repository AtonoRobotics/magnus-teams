//! The admin console's authority and audit (admin console spec, invariant
//! tests, Data and audit): its root database role reads past the walls
//! between people and can add to the log but never change or delete from
//! it; and every console action is written to the log by the kernel, the
//! log's one writer, over its local control socket, before it answers.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use magnus_scratch::admin::*;
use serde_json::{json, Value};

/// One HTTP request over the kernel's control socket.
async fn control(app: &Appliance, method: &str, path: &str, body: Option<Value>) -> Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(app.state_dir.join("control.sock")).await.expect("the kernel's control socket");
    let b = body.map(|b| b.to_string()).unwrap_or_default();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: kernel\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}\n{body}");
    serde_json::from_str(body).unwrap_or_else(|_| panic!("{body}"))
}

fn root_url(app: &Appliance) -> String {
    let mut u = url::Url::parse(&app.db.kernel_url()).unwrap();
    u.set_username(&format!("{}_root", app.db.kernel_role)).unwrap();
    u.set_password(Some("root-pw-1a2b")).unwrap();
    u.to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_consoles_root_role_reads_past_every_wall_and_can_never_change_the_log() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (a, c) = (alice.submit("Draft the board letter.").await, carol.submit("Plan the offsite.").await);
    alice.wait_settled(&a).await;
    carol.wait_settled(&c).await;
    // The console's config names its role; `magnus migrate` makes it.
    let p = prepare(&app, &["198.51.100.0/24"], &[("database_url", root_url(&app).into())]);
    let out = std::process::Command::new(bin("magnus-appliance")).args(["migrate", "--config"]).arg(&p.config).args(["--admin-url", &app.db.admin_url()]).output().unwrap();
    assert!(out.status.success(), "migrate: {}", String::from_utf8_lossy(&out.stderr));

    let root = magnus_invariants::pg::connect(&root_url(&app)).await;
    // Past the walls between people: everyone's content.
    let people: Vec<String> = root.query("SELECT DISTINCT person FROM public.content WHERE person IS NOT NULL ORDER BY 1", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(people, ["alice", "carol"]);
    // The log, read but never changed or deleted from.
    let n: i64 = root.query_one("SELECT count(*) FROM public.events", &[]).await.unwrap().get(0);
    assert!(n > 0);
    for sql in [
        "UPDATE public.events SET actor = 'someone' WHERE seq = 1",
        "DELETE FROM public.events WHERE seq = (SELECT max(seq) FROM public.events)",
        "TRUNCATE public.events",
        "UPDATE public.content SET body = '{}'::jsonb",
        "DELETE FROM public.content",
    ] {
        assert!(root.batch_execute(sql).await.is_err(), "the root role was allowed: {sql}");
    }
    assert_eq!(root.query_one("SELECT count(*) FROM public.events", &[]).await.unwrap().get::<_, i64>(0), n);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_console_action_is_written_to_the_log_by_the_kernel_and_kept() {
    use std::os::unix::fs::PermissionsExt;
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    // The socket is the appliance user's alone.
    let mode = std::fs::metadata(app.state_dir.join("control.sock")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the control socket is {mode:o}");

    let entry = json!({ "admin": "sub-1", "name": "Alice", "action": "user_role_changed", "target": "carol", "source": "10.8.0.5", "before": { "role": "member" }, "after": { "role": "admin" } });
    let seq = control(&app, "POST", "/v1/audit", Some(entry)).await["seq"].as_i64().unwrap();
    let row = app.db.admin().await.query_one("SELECT kind, actor, body FROM public.events WHERE seq = $1", &[&seq]).await.unwrap();
    let (kind, actor, body): (String, String, Value) = (row.get(0), row.get(1), row.get(2));
    assert_eq!((kind.as_str(), actor.as_str()), ("admin_action", "admin:sub-1"));
    assert_eq!(
        (body["action"].as_str(), body["target"].as_str(), body["source"].as_str(), &body["before"]["role"], &body["after"]["role"]),
        (Some("user_role_changed"), Some("carol"), Some("10.8.0.5"), &json!("member"), &json!("admin")),
        "{body}"
    );

    // It is in the audit, filtered by actor, action and target, and kept
    // across a restart, rebuilt from the log.
    app.restart().await;
    for q in ["", "?actor=alice", "?action=role", "?target=carol"] {
        let v = control(&app, "GET", &format!("/v1/audit{q}"), None).await;
        assert!(v["entries"].as_array().unwrap().iter().any(|e| e["seq"] == seq), "{q}: {v}");
    }
    let v = control(&app, "GET", "/v1/audit?target=bob", None).await;
    assert!(v["entries"].as_array().unwrap().is_empty(), "{v}");
}

/// The console's kill switch is the app's own (owner's ruling 10), through
/// the kernel (admin console spec, Kill switch): engaged from the console it
/// suspends running work, refuses new work, says who and when, stays on
/// across a restart, and shows in Overview; released, the app is free again.
#[tokio::test(flavor = "multi_thread")]
async fn the_kill_switch_through_the_kernel_is_the_apps_own_and_outlives_a_restart() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let v = control(&app, "POST", "/v1/kill-switch", Some(json!({ "actor": "admin:sub-1", "engaged": true }))).await;
    assert_eq!(v["engaged"], true, "{v}");
    // The app's own switch is on, moved by the admin.
    assert_eq!(alice.get("/v1/kill-switch").await.1["engaged"], true);
    let s = control(&app, "GET", "/v1/kill-switch", None).await;
    assert_eq!((s["engaged"].as_bool(), s["by"].as_str()), (Some(true), Some("admin:sub-1")), "{s}");
    assert!(s["at"].is_string() && s["tasks_halted"].is_number() && s["schedules_held"].is_number(), "{s}");
    // No new run starts, from anyone, and that holds across a restart.
    app.restart().await;
    assert_eq!(control(&app, "GET", "/v1/kill-switch", None).await["engaged"], true, "the switch did not outlive a restart");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Write a note." })).await;
    assert!(!st.is_success(), "a task started while the switch was on: {v}");
    let o = control(&app, "GET", "/v1/overview", None).await;
    assert_eq!((o["kill_switch"].as_bool(), o["log_up"].as_bool()), (Some(true), Some(true)), "{o}");
    assert!(o["version"].is_string() && o["running_tasks"] == 0 && o["signed_in"].is_number() && o["recent_errors"].is_array(), "{o}");

    let v = control(&app, "POST", "/v1/kill-switch", Some(json!({ "actor": "admin:sub-1", "engaged": false }))).await;
    assert_eq!(v["engaged"], false, "{v}");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Write a note." })).await;
    assert!(st.is_success(), "work did not start again after the release: {v}");
}
