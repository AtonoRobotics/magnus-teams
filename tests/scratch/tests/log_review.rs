//! Regressions from the review of master (2026-10-29), the log and backups:
//! - an append that reached the log without being applied (its caller was
//!   dropped mid-statement) is caught at the next write, and the kernel
//!   starts again from the log;
//! - `magnus restore` refuses while a kernel holds the log;
//! - after a rollback, a backup holding an event of a later release's kind
//!   still passes its restore test, as the log itself still loads.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn an_append_the_kernel_never_applied_is_caught_at_the_next_write() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let first = alice.submit("Say hello.").await;
    alice.wait_settled(&first).await;

    // The owner engaged the kill switch; the insert committed, but the
    // handler was dropped before it applied the event.
    let admin = app.db.admin().await;
    admin
        .execute("INSERT INTO public.events (kind, actor, run_id, body) VALUES ('kill_switch', 'person:alice', NULL, $1)", &[&json!({ "kind": "kill_switch", "engaged": true })])
        .await
        .unwrap();
    assert_eq!(alice.get("/v1/kill-switch").await.1["engaged"], false, "the test needs the event unapplied");

    // The next write notices the gap and the kernel starts again from the
    // log, where the switch is engaged.
    // (Its own answer may be lost as the process starts again.)
    let api = app.api("alice");
    let _ = tokio::spawn(async move { api.post("/v1/tasks", json!({ "text": "Say goodbye." })).await }).await;
    let state = eventually("the kernel applies what is in its log", PATIENCE, || async {
        // Until the process listens again, the request is refused: not yet.
        let r = reqwest::Client::new().get(format!("{}/v1/kill-switch", alice.base)).bearer_auth(&alice.token).send().await.ok()?;
        let v: serde_json::Value = r.error_for_status().ok()?.json().await.ok()?;
        (v["engaged"] == true).then_some(v)
    })
    .await;
    assert_eq!(state["engaged"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_waits_for_no_kernel_and_a_rolled_back_log_still_backs_up() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let t = alice.submit("Say hello.").await;
    alice.wait_settled(&t).await;

    // A later release wrote a kind this one does not know, then the
    // appliance went back one release: the log loads, skipping it...
    app.kill().await;
    let admin = app.db.admin().await;
    admin
        .execute("INSERT INTO public.events (kind, actor, run_id, body) VALUES ('from_a_later_release', 'kernel', NULL, $1)", &[&json!({ "kind": "from_a_later_release", "x": 1 })])
        .await
        .unwrap();
    app.restart().await;
    // ...and so does its backup's restore test.
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    assert_eq!(b["restored"], true, "a rolled-back log's backup fails its restore test: {b}");
    let file = app.state_dir.join("backups").join(b["file"].as_str().unwrap());

    // The log and the vault are lost; a kernel (here, its lock) is running
    // on the empty log, so the restore is refused.
    app.kill().await;
    admin.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    let (config, url) = (app.dir.path().join("magnus.toml"), app.db.admin_url());
    let restore =
        || std::process::Command::new(bin("magnus-appliance")).args(["restore", "--config"]).arg(&config).args(["--admin-url", &url, "--from"]).arg(&file).output().unwrap();
    let lock = app.db.admin().await;
    assert!(lock.query_one("SELECT pg_try_advisory_lock(7239001)", &[]).await.unwrap().get::<_, bool>(0));
    let out = restore();
    assert!(!out.status.success(), "a restore ran under a live kernel");
    assert!(String::from_utf8_lossy(&out.stderr).contains("a kernel is running"), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!app.state_dir.join("vault/secrets.json").exists(), "a refused restore wrote the vault");
    drop(lock);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let out = restore();
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    app.restart().await;
    assert_eq!(alice.task(&t).await["status"], "failed");
}

/// An event a kernel wrote whole, before `migrate` made the content store,
/// keeps its content in the log, where nothing can delete it. `migrate`
/// moves it to the content store, leaving the record with its hash, as the
/// kernel writes events now; the event reads back as it was written, and a
/// second `migrate` changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn migrate_moves_content_written_into_the_log_to_the_content_store() {
    let db = pg::TestDb::create().await;
    magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect("migrate");
    let admin = db.admin().await;
    let whole = json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "carol", "text": "Carol's private request.", "config": {} });
    let seq: i64 =
        admin.query_one("INSERT INTO public.events (kind, actor, run_id, body) VALUES ('task_created', 'person:carol', 'r1', $1) RETURNING seq", &[&whole]).await.unwrap().get(0);

    for _ in 0..2 {
        magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect("migrate");
        let record: serde_json::Value = admin.query_one("SELECT body FROM public.events WHERE seq = $1", &[&seq]).await.unwrap().get(0);
        assert!(record.get("text").is_none() && record["content"]["sha256"].is_string(), "the content is still in the log: {record}");
        let rows = admin.query("SELECT body, person FROM public.content WHERE seq = $1", &[&seq]).await.unwrap();
        assert_eq!(rows.len(), 1, "the content was not moved, or moved twice");
        let (content, person): (serde_json::Value, Option<String>) = (rows[0].get(0), rows[0].get(1));
        assert_eq!((content["text"].as_str(), person.as_deref()), (Some("Carol's private request."), Some("carol")));
        assert_eq!(magnus::log::join(record, Some(content)), whole, "the event no longer reads as written");
    }
}

/// A password with characters a URL must escape (`@`, `:`, `/`, `%`) is
/// given to the role as the kernel will send it, decoded, not as written in
/// the URL, so the kernel can still sign in after `migrate`.
#[tokio::test(flavor = "multi_thread")]
async fn migrate_sets_the_password_the_kernel_signs_in_with() {
    let db = pg::TestDb::create().await;
    let mut u = url::Url::parse(&db.kernel_url()).unwrap();
    u.set_password(Some("p@ss:w/rd%41'")).unwrap();
    assert!(u.as_str().contains("%40"), "the URL escapes the password: {u}");
    magnus::log::migrate(&db.admin_url(), u.as_str(), None).await.expect("migrate");
    let kernel = magnus::log::connect(u.as_str()).await.expect("the kernel signs in with the password migrate set");
    kernel.execute("SELECT 1", &[]).await.unwrap();
}

/// The log is append-only because the database says so (DECISIONS, Event
/// log and state), whoever made the kernel's role. `migrate` takes away
/// what a role made beforehand may hold (superuser and the like), and
/// refuses when the role can still change or delete the log, here through
/// membership in `pg_write_all_data`.
#[tokio::test(flavor = "multi_thread")]
async fn migrate_leaves_the_kernel_role_unable_to_change_the_log() {
    // Made beforehand as a superuser: migrate takes that away.
    let db = pg::TestDb::create().await;
    let admin = db.admin().await;
    admin.batch_execute(&format!("CREATE ROLE {} LOGIN SUPERUSER PASSWORD 'x'", db.kernel_role)).await.unwrap();
    magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect("migrate");
    let kernel = db.as_kernel().await;
    kernel.execute("INSERT INTO public.events (kind, actor, body) VALUES ('x', 'test', '{}')", &[]).await.unwrap();
    assert!(kernel.execute("UPDATE public.events SET kind = 'y'", &[]).await.is_err(), "the kernel's role can change the log");
    assert!(kernel.execute("DELETE FROM public.events", &[]).await.is_err(), "the kernel's role can delete from the log");

    // Granted a way past the table's grants: migrate refuses.
    let db = pg::TestDb::create().await;
    let admin = db.admin().await;
    admin.batch_execute(&format!("CREATE ROLE {} LOGIN PASSWORD 'x'; GRANT pg_write_all_data TO {}", db.kernel_role, db.kernel_role)).await.unwrap();
    let err = magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect_err("migrate left a role that can change the log");
    assert!(format!("{err:#}").contains("change the log"), "{err:#}");
}

/// A role made beforehand with NOINHERIT holds nothing through its
/// memberships until it switches to one with SET ROLE, so it passed the
/// check above and could then change the log (#186). `migrate` refuses any
/// membership the role can switch to, directly or through roles in between,
/// that can change the log or holds what the kernel's role is stripped of;
/// once the membership is taken away, no role the kernel can switch to can
/// change or delete the log.
#[tokio::test(flavor = "multi_thread")]
async fn migrate_refuses_a_kernel_role_that_can_set_role_to_change_the_log() {
    // Directly, to pg_write_all_data; through a role in between; to a superuser.
    for (setup, named) in [
        ("GRANT pg_write_all_data TO {k}", "pg_write_all_data"),
        ("CREATE ROLE {k}_mid NOLOGIN; GRANT pg_write_all_data TO {k}_mid; GRANT {k}_mid TO {k}", "pg_write_all_data"),
        ("CREATE ROLE {k}_su NOLOGIN SUPERUSER; GRANT {k}_su TO {k}", "{k}_su"),
    ] {
        let db = pg::TestDb::create().await;
        let k = db.kernel_role.clone();
        let (setup, named) = (setup.replace("{k}", &k), named.replace("{k}", &k));
        let admin = db.admin().await;
        admin.batch_execute(&format!("CREATE ROLE {k} LOGIN NOINHERIT PASSWORD 'x'; {setup}")).await.unwrap();
        let err = magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None)
            .await
            .expect_err(&format!("migrate left a NOINHERIT role that can SET ROLE to {named} and change the log ({setup})"));
        let msg = format!("{err:#}");
        assert!(msg.contains("change the log") && msg.contains(&named), "the refusal does not name {named}: {msg}");

        // The route is real: the kernel, switching role, changes the log.
        admin.execute("INSERT INTO public.events (kind, actor, body) VALUES ('x', 'test', '{}')", &[]).await.unwrap();
        let kernel = db.as_kernel().await;
        kernel.batch_execute(&format!("SET ROLE {named}")).await.unwrap();
        kernel.execute("UPDATE public.events SET kind = 'y'", &[]).await.expect("the test needs the route to the log to be real");
    }

    // Taken away as the refusal says, migrate passes, and no role the
    // kernel can switch to can change or delete the log.
    let db = pg::TestDb::create().await;
    let k = db.kernel_role.clone();
    let admin = db.admin().await;
    admin.batch_execute(&format!("CREATE ROLE {k} LOGIN NOINHERIT PASSWORD 'x'; GRANT pg_write_all_data TO {k}")).await.unwrap();
    magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect_err("migrate refuses the membership");
    admin.batch_execute(&format!("REVOKE pg_write_all_data FROM {k}")).await.unwrap();
    magnus::log::migrate(&db.admin_url(), &db.kernel_url(), None).await.expect("migrate");
    let kernel = db.as_kernel().await;
    kernel.execute("INSERT INTO public.events (kind, actor, body) VALUES ('x', 'test', '{}')", &[]).await.unwrap();
    assert!(kernel.batch_execute("SET ROLE pg_write_all_data").await.is_err(), "the kernel can still switch to pg_write_all_data");
    let settable: Vec<String> =
        kernel.query("SELECT rolname FROM pg_roles WHERE pg_has_role(current_user, oid, 'SET') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(settable.len() > 1, "the kernel's role is a member of its readers: {settable:?}");
    for r in &settable {
        kernel.batch_execute(&format!("SET ROLE \"{r}\"")).await.unwrap();
        assert!(kernel.execute("UPDATE public.events SET kind = 'y'", &[]).await.is_err(), "as {r} the kernel can change the log");
        assert!(kernel.execute("DELETE FROM public.events", &[]).await.is_err(), "as {r} the kernel can delete from the log");
        assert!(kernel.batch_execute("TRUNCATE public.events").await.is_err(), "as {r} the kernel can empty the log");
        kernel.batch_execute("RESET ROLE").await.unwrap();
    }
}
