//! Signed appliance updates with one-step rollback (gate item 5). The
//! appliance runs from its own install directory; releases arrive in its
//! updates directory, signed as `tauri signer sign --app-version` signs them.
//! A release signed by another key, or announced as a version it was not
//! signed as, or unable to read this appliance's log, is not installed. A
//! good one is installed while a task runs: the task is suspended for the
//! switch and resumed by the new release, and nothing in it is done twice.
//! The owner goes back one release; a release that cannot serve goes back by
//! itself; a release gone back from is not installed again. App and
//! appliance exchange versions, and an app older than the appliance accepts
//! is told to update.

use base64::Engine;
use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;

static SWITCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

struct Key {
    kp: minisign::KeyPair,
}

impl Key {
    fn new() -> Key {
        Key { kp: minisign::KeyPair::generate_unencrypted_keypair().unwrap() }
    }
    /// The public key as the updater's configuration holds it.
    fn public(&self) -> String {
        B64.encode(self.kp.pk.to_box().unwrap().to_string())
    }
    /// A signature bound to `version`, as `tauri signer sign --app-version` makes it.
    fn sign(&self, data: &[u8], version: &str) -> String {
        let trusted = format!("timestamp:{}\tfile:release.tar.gz\tversion:{version}", chrono::Utc::now().timestamp());
        let sig = minisign::sign(Some(&self.kp.pk), &self.kp.sk, std::io::Cursor::new(data), Some(&trusted), Some("signature from tauri secret key")).unwrap();
        B64.encode(sig.to_string())
    }
}

/// A release package: the given files and its VERSION, gzipped tar.
fn package(version: &str, files: &[(&str, &Path)]) -> Vec<u8> {
    let mut t = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let mut add = |name: &str, data: &[u8], mode: u32| {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_cksum();
        t.append_data(&mut h, name, data).unwrap();
    };
    add("VERSION", format!("{version}\n").as_bytes(), 0o644);
    for (name, path) in files {
        add(name, &std::fs::read(path).unwrap(), 0o755);
    }
    t.into_inner().unwrap().finish().unwrap()
}

/// Publish a release into the appliance's updates directory.
fn publish(updates: &Path, announced: &str, bytes: &[u8], signature: String) {
    let dir = updates.join("appliance");
    std::fs::create_dir_all(&dir).unwrap();
    let file = format!("magnus-appliance-{announced}.tar.gz");
    std::fs::write(dir.join(&file), bytes).unwrap();
    let m = json!({ "version": announced, "notes": format!("Release {announced}."), "file": file, "signature": signature });
    std::fs::write(dir.join("latest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
}

/// The appliance as installed: its programs in their own directory, which
/// an update swaps, run as one process the whole time (an update replaces
/// the program in place).
struct Installed {
    child: tokio::process::Child,
    base: String,
}

impl Installed {
    async fn start(inst: &Path, config: &Path, logs: &Path, base: &str) -> Installed {
        let log = |k: &str| std::fs::OpenOptions::new().create(true).append(true).open(logs.join(format!("installed.{k}"))).unwrap();
        let child = tokio::process::Command::new(inst.join("magnus-appliance"))
            .args(["serve", "--config"])
            .arg(config)
            .stdout(Stdio::from(log("out")))
            .stderr(Stdio::from(log("err")))
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut me = Installed { child, base: base.to_string() };
        me.release(&logs.join("installed.err")).await;
        me
    }

    /// The release serving now, once it serves.
    async fn release(&mut self, log: &Path) -> String {
        let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap();
        let start = std::time::Instant::now();
        loop {
            if let Ok(Some(st)) = self.child.try_wait() {
                panic!("the appliance exited with {st}:\n{}", std::fs::read_to_string(log).unwrap_or_default());
            }
            if let Ok(r) = http.get(format!("{}/v1/health", self.base)).send().await {
                if let Ok(v) = r.json::<Value>().await {
                    return v["release"].as_str().unwrap().to_string();
                }
            }
            assert!(start.elapsed() < PATIENCE, "the appliance is not serving:\n{}", std::fs::read_to_string(log).unwrap_or_default());
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

/// The owner's update status once `f` holds; else the appliance's log.
async fn wait_for(alice: &Api, log: &Path, what: &str, f: impl Fn(&Value) -> bool) -> Value {
    let start = std::time::Instant::now();
    let mut last = Value::Null;
    while start.elapsed() < PATIENCE {
        // Between releases nothing answers.
        let r =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap().get(format!("{}/v1/updates", alice.base)).bearer_auth(&alice.token).send().await;
        if let Ok(r) = r {
            let ok = r.status().is_success();
            let v: Value = r.json().await.unwrap_or_default();
            if ok && f(&v) {
                return v;
            }
            last = v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let log = std::fs::read_to_string(log).unwrap_or_default();
    panic!("timed out waiting for: {what}\nlast status: {last}\nappliance log:\n{}", &log[log.len().saturating_sub(6000)..]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signed_release_installs_while_a_task_runs_and_goes_back_one_step() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("read the inbox", "the inbox was read"),
            1 => tool("mail__list_inbox", json!({})),
            // The reply in flight when the update comes: it returns once the
            // appliance is switching releases, or after PATIENCE, so an
            // update that never starts fails the test's own waits below by
            // name and never holds the reply, and the run, open-ended (run
            // 41782499090 held this test for more than ten minutes).
            2 => {
                let start = std::time::Instant::now();
                while !SWITCHING.load(std::sync::atomic::Ordering::SeqCst) && start.elapsed() < PATIENCE {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                complete("read")
            }
            _ => complete("read"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]), &m).await;
    let alice = app.api("alice");
    app.kill().await;

    // Installed: the programs in their own directory, at release 0.1.0.
    let root = app.dir.path().to_path_buf();
    let inst = root.join("programs");
    std::fs::create_dir_all(&inst).unwrap();
    std::fs::copy(bin("magnus-appliance"), inst.join("magnus-appliance")).unwrap();
    std::fs::write(inst.join("VERSION"), "0.1.0\n").unwrap();
    let updates = root.join("updates");
    let key = Key::new();
    let config = root.join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    cfg.insert("updates".into(), toml::Value::Table(toml::toml! { check_minutes = 0 }));
    cfg["updates"].as_table_mut().unwrap().insert("dir".into(), updates.display().to_string().into());
    cfg["updates"].as_table_mut().unwrap().insert("public_key".into(), key.public().into());
    std::fs::write(&config, toml::to_string(&cfg).unwrap()).unwrap();
    // The harness was still listening, so this process could not bind that
    // port and a health check waited forever on a connection nobody read.
    app.release_port();
    let err_log = root.join("installed.err");
    let mut a = Installed::start(&inst, &config, &root, &app.base).await;
    let pid = a.child.id();
    assert_eq!(a.release(&err_log).await, "0.1.0");

    // Versions both ways; an app older than the appliance accepts is told to update.
    let (_, v) = alice.get("/v1/version").await;
    let min_app = v["min_app"].as_str().unwrap().to_string();
    assert_eq!(v["version"], "0.1.0", "{v}");
    let http = reqwest::Client::new();
    let as_app = |ver: &str| http.get(format!("{}/v1/me", app.base)).bearer_auth(&alice.token).header("x-magnus-app", ver).send();
    let old = as_app("0.4.5").await.unwrap();
    assert_eq!(old.status(), 426);
    assert_eq!(old.headers()["x-magnus-appliance"], "0.1.0");
    let old: Value = old.json().await.unwrap();
    assert_eq!((old["update_required"].clone(), old["min_app"].as_str()), (json!(true), Some(min_app.as_str())), "{old}");
    assert!(as_app(&min_app).await.unwrap().status().is_success());

    // Only the owner updates.
    assert_eq!(app.api("carol").post("/v1/updates/check", json!({})).await.0, 403);
    let program = inst.join("magnus-appliance");

    // Signed by another key: not installed.
    let good = package("0.1.1", &[("magnus-appliance", &program)]);
    publish(&updates, "0.1.1", &good, Key::new().sign(&good, "0.1.1"));
    assert_eq!(alice.post("/v1/updates/check", json!({})).await.0, 202);
    let v = wait_for(&alice, &err_log, "the forged release is refused", |v| v["last"]["what"] == "failed").await;
    assert!(v["last"]["detail"].as_str().unwrap().contains("does not verify"), "{v}");

    // Signed as 0.1.1 and announced as 0.2.0: not installed.
    publish(&updates, "0.2.0", &good, key.sign(&good, "0.1.1"));
    alice.post("/v1/updates/check", json!({})).await;
    let v = wait_for(&alice, &err_log, "the relabelled release is refused", |v| v["last"]["to"] == "0.2.0").await;
    assert!(v["last"]["detail"].as_str().unwrap().contains("signed as 0.1.1"), "{v}");

    // A release that cannot read this appliance's log is not switched to.
    let unreadable = root.join("unreadable.sh");
    std::fs::write(&unreadable, "#!/bin/sh\necho 'unknown event kind' >&2\nexit 1\n").unwrap();
    let bad = package("0.1.1", &[("magnus-appliance", &unreadable)]);
    publish(&updates, "0.1.1", &bad, key.sign(&bad, "0.1.1"));
    alice.post("/v1/updates/check", json!({})).await;
    let v = wait_for(&alice, &err_log, "the unreadable release is refused", |v| v["last"]["to"] == "0.1.1").await;
    assert!(v["last"]["detail"].as_str().unwrap().contains("cannot read this appliance"), "{v}");
    assert_eq!(a.release(&err_log).await, "0.1.0");

    // A good release while a task is mid-reply.
    let task = alice.submit("Read my inbox.").await;
    eventually("the task's last reply is in flight", PATIENCE, || async { (m.requests().len() == 3).then_some(()) }).await;
    publish(&updates, "0.1.1", &good, key.sign(&good, "0.1.1"));
    let (st, v) = alice.post("/v1/updates/check", json!({})).await;
    assert_eq!((st.as_u16(), v["latest"].clone()), (202, json!("0.1.1")), "{v}");
    wait_for(&alice, &err_log, "the switch has begun", |v| v["updating"]["to"] == "0.1.1").await;
    SWITCHING.store(true, std::sync::atomic::Ordering::SeqCst);
    let v = wait_for(&alice, &err_log, "0.1.1 is running", |v| v["version"] == "0.1.1" && v["last"]["what"] == "updated").await;
    assert_eq!((v["last"]["from"].clone(), v["previous"].clone()), (json!("0.1.0"), json!("0.1.0")), "{v}");
    assert_eq!(a.child.id(), pid, "the update ran as another process");

    // The task went on: suspended for the switch, resumed by the new release, nothing twice.
    let done = alice.wait_settled(&task).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(mail.calls_to("list_inbox").len(), 1, "a tool call was repeated");
    // The memory pass reads the ended task with a request of its own.
    let task_requests = m.requests().into_iter().filter(|c| !c.body["system"].to_string().contains("You keep the person's long-term memory")).count();
    assert_eq!(task_requests, 3, "a model reply was asked for again");
    let db = app.db.admin().await;
    let kinds: Vec<(String, Value)> = db
        .query("SELECT kind, body FROM public.events WHERE run_id = (SELECT run_id FROM public.events WHERE kind = 'task_created' ORDER BY seq DESC LIMIT 1) ORDER BY seq", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let at = |k: &str| kinds.iter().position(|(kind, b)| kind == k && (k != "suspended" || b["reason"] == "update"));
    assert!(
        matches!((at("suspended"), at("resumed")), (Some(s), Some(r)) if s < r),
        "the task was not suspended and resumed for the update: {:?}",
        kinds.iter().map(|(k, b)| format!("{k}:{}", b["reason"])).collect::<Vec<_>>()
    );

    // The owner goes back one step; the release gone back from is not installed again.
    let (st, v) = alice.post("/v1/updates/rollback", json!({})).await;
    assert_eq!((st.as_u16(), v["to"].clone()), (202, json!("0.1.0")), "{v}");
    let v = wait_for(&alice, &err_log, "0.1.0 is running again", |v| v["version"] == "0.1.0" && v["last"]["what"] == "rolled_back").await;
    assert_eq!(v["skipped"], json!(["0.1.1"]), "{v}");
    let (st, v) = alice.post("/v1/updates/check", json!({})).await;
    assert_eq!((st.as_u16(), v["status"].clone()), (200, json!("up_to_date")), "{v}");

    // A release that starts and cannot serve goes back by itself.
    let broken = root.join("broken.sh");
    let broken_cfg = root.join("broken.toml");
    let mut bc = cfg.clone();
    // A port this test holds, so the listen fails for root too (port 1 does
    // not refuse root).
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = held.local_addr().unwrap().to_string();
    bc["appliance"].as_table_mut().unwrap().insert("listen".into(), taken.clone().into());
    std::fs::write(&broken_cfg, toml::to_string(&bc).unwrap()).unwrap();
    std::fs::write(&broken, format!("#!/bin/sh\nd=$(dirname \"$0\")\n[ \"$1\" = serve ] && exec \"$d/real\" serve --config {}\nexec \"$d/real\" \"$@\"\n", broken_cfg.display()))
        .unwrap();
    let release = package("0.1.2", &[("magnus-appliance", &broken), ("real", &program)]);
    publish(&updates, "0.1.2", &release, key.sign(&release, "0.1.2"));
    alice.post("/v1/updates/check", json!({})).await;
    let v = wait_for(&alice, &err_log, "the broken release is gone back from", |v| v["last"]["what"] == "rolled_back" && v["last"]["from"] == "0.1.2").await;
    assert_eq!(v["version"], "0.1.0", "{v}");
    assert!(v["last"]["detail"].as_str().unwrap().contains(&format!("listen on {taken}")), "{v}");
    assert_eq!(v["skipped"], json!(["0.1.1", "0.1.2"]), "{v}");
    assert_eq!(a.release(&err_log).await, "0.1.0");
    assert_eq!(a.child.id(), pid);
    let t = alice.submit("Read my inbox again.").await;
    assert_eq!(alice.wait_settled(&t).await["status"], "completed");

    // The release kept now is the broken one. The owner goes back to it
    // anyway: it reads the appliance but cannot serve, so the appliance
    // comes back to the release that was running, instead of staying down.
    let count = |kind: &'static str| {
        let db = &db;
        async move { db.query_one("SELECT count(*) FROM public.events WHERE kind = $1", &[&kind]).await.unwrap().get::<_, i64>(0) }
    };
    let back_before = count("update_rolled_back").await;
    let (st, v) = alice.post("/v1/updates/rollback", json!({})).await;
    assert_eq!((st.as_u16(), v["to"].clone()), (202, json!("0.1.2")), "{v}");
    eventually("the appliance is back on 0.1.0", PATIENCE, || async { (count("update_rolled_back").await > back_before).then_some(()) }).await;
    assert_eq!(a.release(&err_log).await, "0.1.0");
    assert_eq!(a.child.id(), pid, "the appliance stopped");
    drop(a);
    drop(held);
    let _: PathBuf = inst;
}
