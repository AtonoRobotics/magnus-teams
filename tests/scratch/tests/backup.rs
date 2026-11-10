//! Backup and tested restore of the event log and the vault (K10): a
//! backup is encrypted, restored in memory as soon as it is written, kept
//! no longer than its keeping time, and after the log and the vault are
//! lost, `magnus restore` brings back the appliance's history and secrets.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_restores_the_log_and_the_vault_after_both_are_lost() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("read the inbox", "the inbox was read"),
            1 => tool("mail__list_inbox", json!({})),
            _ => complete("read"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]), &m).await;
    let alice = app.api("alice");
    let words = canary("task-words");
    let t1 = alice.submit(&format!("Read my inbox. {words}")).await;
    alice.wait_settled(&t1).await;
    assert!(alice.put_policy("user/alice", r#"forbid(principal, action, resource == Tool::"mail__send");"#).await.is_success());

    // Only the owner backs up; an expired backup goes when a new one is made.
    assert_eq!(app.api("carol").post("/v1/backups", json!({})).await.0, 403);
    let dir = app.state_dir.join("backups");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("magnus-20200101T000000.000Z.backup"), b"old").unwrap();
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    assert_eq!(b["restored"], true, "the backup did not restore in memory: {b}");
    assert_eq!(b["removed"], json!(["magnus-20200101T000000.000Z.backup"]), "{b}");
    let (_, list) = alice.get("/v1/backups").await;
    assert_eq!(list["latest"]["restored"], true, "{list}");
    assert_eq!(list["files"].as_array().unwrap().len(), 1, "{list}");

    // Encrypted: neither the person's words nor a secret is in the file.
    let file = dir.join(b["file"].as_str().unwrap());
    let raw = std::fs::read(&file).unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(!text.contains(&words) && !text.contains(&app.app_secrets["mail"]), "the backup is readable");

    let before = alice.tasks().await;
    // The disaster: the log, its content and the vault are gone. The backups and their key are kept.
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();

    let (config, admin) = (app.dir.path().join("magnus.toml"), app.db.admin_url());
    let restore = |from: &std::path::Path| {
        std::process::Command::new(bin("magnus-appliance")).args(["restore", "--config"]).arg(&config).args(["--admin-url", &admin, "--from"]).arg(from).output().unwrap()
    };
    let out = restore(&file);
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    app.restart().await;

    // The history is back, and so are the policy and the app's credential.
    let after = alice.tasks().await;
    assert_eq!(before, after, "the tasks differ after the restore");
    let (_, p) = alice.get("/v1/policies").await;
    assert!(p.to_string().contains("mail__send"), "the policy was not restored: {p}");
    let t2 = alice.submit("Read my inbox again.").await;
    let done = alice.wait_settled(&t2).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(mail.calls_to("list_inbox").len(), 2, "the app's credential did not come back");

    // A restore never writes over a log that has history.
    let out = restore(&file);
    assert!(!out.status.success(), "a restore wrote over a live log");
}

/// The appliance's identity provider keeps its accounts in a database of its
/// own (#290); a backup holds it too, so losing it with the log does not
/// lose everyone's sign-in (#296).
#[tokio::test(flavor = "multi_thread")]
async fn a_database_the_appliance_depends_on_comes_back_from_the_backup() {
    let m = FakeModel::start(script(|_| async move { complete("done") })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let idp = magnus_invariants::pg::TestDb::create().await;
    idp.admin().await.batch_execute("CREATE TABLE realm (name text); INSERT INTO realm VALUES ('magnus')").await.unwrap();
    let config = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = toml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    let mut backup = toml::Table::new();
    backup.insert("databases".into(), toml::Value::Array(vec![idp.admin_url().into()]));
    cfg.insert("backup".into(), backup.into());
    std::fs::write(&config, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;

    let alice = app.api("alice");
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    assert_eq!(b["restored"], true, "{b}");
    assert!(b["detail"].as_str().unwrap_or_default().contains("1 databases read back"), "{b}");

    // The disaster takes the log, the vault and the identity provider's data.
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    idp.admin().await.batch_execute("DROP TABLE realm").await.unwrap();

    let file = app.state_dir.join("backups").join(b["file"].as_str().unwrap());
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(&config)
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    let name: String = idp.admin().await.query_one("SELECT name FROM realm", &[]).await.unwrap().get(0);
    assert_eq!(name, "magnus", "the identity provider's data did not come back");
}

/// The appliance's own accounts ride the backup too: after the log, the
/// vault and the accounts tables are lost, `magnus restore` brings back
/// everyone's sign-in (#466).
#[tokio::test(flavor = "multi_thread")]
async fn the_appliances_accounts_come_back_from_the_backup() {
    let m = FakeModel::start(script(|_| async move { complete("done") })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let config = app.dir.path().join("magnus.toml");
    let cli = |args: &[&str], stdin: Option<&str>| {
        let mut c = std::process::Command::new(bin("magnus-appliance"));
        c.args(args).arg("--config").arg(&config);
        if stdin.is_some() {
            c.stdin(std::process::Stdio::piped());
        }
        let mut c = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        if let Some(text) = stdin {
            use std::io::Write;
            writeln!(c.stdin.take().unwrap(), "{text}").unwrap();
        }
        c.wait_with_output().unwrap()
    };
    let out = cli(&["person-add", "--person", "ada", "--name", "Ada"], None);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    let out = cli(&["person-role", "--person", "ada", "--role", "admin"], None);
    assert!(out.status.success(), "magnus person-role: {}", String::from_utf8_lossy(&out.stderr));
    let out = cli(&["password", "--person", "ada"], Some("ada-long-password"));
    assert!(out.status.success(), "magnus password: {}", String::from_utf8_lossy(&out.stderr));
    async fn sign_in(base: &str) -> (reqwest::StatusCode, Value) {
        let r = reqwest::Client::new()
            .post(format!("{base}/v1/sign-in"))
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "person": "ada", "password": "ada-long-password", "client": "app" }))
            .send()
            .await
            .unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }
    // No restart: the sign-in routing sees the CLI-added person straight
    // from the store, not the boot-time cache (#466).
    let (st, v) = sign_in(&app.base).await;
    assert_eq!(st, 200, "ada cannot sign in before the backup: {v}");

    let alice = app.api("alice");
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    assert_eq!(b["restored"], true, "{b}");
    assert!(b["detail"].as_str().unwrap_or_default().contains("the accounts tables read back"), "{b}");

    // The disaster takes the log, the vault and the accounts tables.
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events, public.account_people, public.account_passwords, public.account_totp, public.account_totp_steps, public.account_statuses, public.account_roles, public.account_invites, public.account_sessions, public.account_machine_codes").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();

    let file = app.state_dir.join("backups").join(b["file"].as_str().unwrap());
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(&config)
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    app.restart().await;

    // Her password signs her in again, and she is still an admin.
    let (st, v) = sign_in(&app.base).await;
    assert_eq!(st, 200, "ada cannot sign in after the restore: {v}");
    let admin = app.db.admin().await;
    let rows: i64 = admin.query_one("SELECT COUNT(*) FROM public.account_people WHERE person = 'ada'", &[]).await.unwrap().get(0);
    assert_eq!(rows, 1, "her record did not come back");
    let role: String = admin.query_one("SELECT role FROM public.account_roles WHERE person = 'ada'", &[]).await.unwrap().get(0);
    assert_eq!(role, "admin", "her role did not come back");
}

/// After each backup and prune the backups directory is mirrored to the
/// configured off-host destination (#297): new files arrive, pruned ones
/// leave; a failed copy is recorded, not a failed backup.
#[tokio::test(flavor = "multi_thread")]
async fn backups_are_mirrored_off_host_after_each_backup_and_prune() {
    let m = FakeModel::start(script(|_| async move { Reply::Text(String::new()) })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    // None configured: the backup holds no copy either way.
    let (st, b) = app.api("alice").post("/v1/backups", json!({})).await;
    assert_eq!(st.as_u16(), 201, "{b}");
    assert!(b["copied"].is_null(), "{b}");

    let offhost = app.dir.path().join("offhost");
    std::fs::create_dir_all(&offhost).unwrap();
    // Junk off host that the mirror drops, as it drops pruned backups.
    std::fs::write(offhost.join("stale.backup"), b"stale").unwrap();
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("copy_to".into(), offhost.to_str().unwrap().into());
        c.insert("backup".into(), toml::Value::Table(t));
    })
    .await;
    let alice = app.api("alice");
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!((st.as_u16(), b["copied"].clone()), (201, Value::Bool(true)), "{b}");
    let file = b["file"].as_str().unwrap().to_string();
    assert!(offhost.join(&file).exists(), "the backup was not mirrored: {b}");
    assert!(!offhost.join("stale.backup").exists(), "the mirror left the stale file");
    let (_, list) = alice.get("/v1/backups").await;
    assert_eq!((list["copy_to"].clone(), list["copy"]["copied"].clone()), (json!(offhost.to_str().unwrap()), Value::Bool(true)), "{list}");

    // A destination that fails: the backup still holds, the copy recorded.
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("copy_to".into(), "/proc/no-such-place/backups".into());
        c.insert("backup".into(), toml::Value::Table(t));
    })
    .await;
    let alice = app.api("alice");
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!((st.as_u16(), b["restored"].clone(), b["copied"].clone()), (201, Value::Bool(true), Value::Bool(false)), "{b}");
    assert!(!b["copy_detail"].as_str().unwrap_or_default().is_empty(), "{b}");
    let logged = app.db.admin().await.query("SELECT body::text FROM events WHERE kind = 'backup_copied' ORDER BY seq", &[]).await.unwrap();
    assert_eq!(logged.len(), 2);
    let last: Value = serde_json::from_str(logged[1].get(0)).unwrap();
    assert_eq!((last["ok"].clone(), last["file"].clone()), (Value::Bool(false), b["file"].clone()), "{last}");
}
