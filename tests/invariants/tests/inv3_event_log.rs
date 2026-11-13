//! Invariant 3: the append-only event log is the source of truth for what
//! happened; the content it refers to is kept beside it and can be deleted,
//! and once deleted it is gone. A crashed run resumes without repeating side
//! effects.
//!
//! Prevents: lost sessions on crash, retries that repeat a side effect after a
//! lost response, and content that can never be deleted.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

fn send() -> Reply {
    tool("mail__send", json!({ "to": "bob@corp.example", "subject": "report", "body": "attached" }))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_kernel_cannot_rewrite_the_log() {
    let model = FakeModel::start(steps(vec![plan("nothing"), fail("done")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Do nothing.").await;
    alice.wait_settled(&id).await;

    let admin = app.db.admin().await;
    let count = |c: tokio_postgres::Client| async move { c.query_one("SELECT count(*) FROM public.events", &[]).await.unwrap().get::<_, i64>(0) };
    let before = count(app.db.admin().await).await;
    assert!(before > 0, "the run left no events");
    let col: String = admin
        .query_one(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_schema = 'public' AND table_name = 'events' ORDER BY ordinal_position LIMIT 1",
            &[],
        )
        .await
        .unwrap()
        .get(0);

    let k = app.db.as_kernel().await;
    for sql in [
        format!("UPDATE public.events SET {col} = {col}"),
        "DELETE FROM public.events".to_string(),
        "TRUNCATE public.events".to_string(),
        "DROP TABLE public.events".to_string(),
        "ALTER TABLE public.events DISABLE TRIGGER ALL".to_string(),
    ] {
        assert!(k.batch_execute(&sql).await.is_err(), "the kernel role may run: {sql}");
    }
    assert_eq!(count(app.db.admin().await).await, before, "events changed");
}

#[tokio::test(flavor = "multi_thread")]
async fn content_is_kept_beside_the_log_not_in_it() {
    let model = FakeModel::start(steps(vec![plan("nothing"), fail("done")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Plan the walrus migration for the third quarter.").await;
    alice.wait_settled(&id).await;

    let admin = app.db.admin().await;
    let all = |table: &'static str| {
        let admin = &admin;
        async move {
            let rows = admin.query(&format!("SELECT row_to_json(t)::text FROM public.{table} t"), &[]).await.unwrap();
            rows.iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>().join("\n")
        }
    };
    assert!(!all("events").await.contains("walrus"), "the task's text is in the event log");
    assert!(all("content").await.contains("walrus"), "the task's text is not in the content store");
    let task = alice.task(&id).await;
    assert!(task.to_string().contains("walrus"), "the task does not read its text: {task}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_items_content_is_gone_and_its_record_stays() {
    let model = FakeModel::start(steps(vec![plan("nothing"), fail("done")])).await;
    let mut app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Plan the walrus migration for the third quarter.").await;
    alice.wait_settled(&id).await;

    assert_eq!(alice.post("/v1/trash", json!({ "item": id })).await.0, 204);
    assert_eq!(alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{id}"), None).await.0, 204);

    let admin = app.db.admin().await;
    let all = |table: &'static str| {
        let admin = &admin;
        async move {
            let rows = admin.query(&format!("SELECT row_to_json(t)::text FROM public.{table} t"), &[]).await.unwrap();
            rows.iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>().join("\n")
        }
    };
    assert!(!all("content").await.contains("walrus"), "the removed task's text is still in the store");
    let created: Value = admin.query_one("SELECT body FROM public.events WHERE kind = 'task_created' AND body->>'task' = $1", &[&id]).await.unwrap().get(0);
    assert!(created["content"]["sha256"].is_string(), "the record lost its content's hash: {created}");
    let removed: i64 = admin.query_one("SELECT count(*) FROM public.events WHERE kind = 'removed' AND body->>'item' = $1", &[&id]).await.unwrap().get(0);
    assert_eq!(removed, 1, "the removal is not recorded");

    // State rebuilds without the content, and the task stays gone.
    app.restart().await;
    let (st, _) = alice.get(&format!("/v1/tasks/{id}")).await;
    assert_eq!(st, 404, "the removed task came back");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_task_cannot_be_deleted() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![plan("send the report"), send(), complete("sent")])).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &model).await;
    let alice = app.api("alice");
    // No permit for send: the run waits for approval, not ended.
    let id = alice.submit("Email bob the report.").await;
    alice.wait_item(&id, "approval").await;

    assert_eq!(alice.post("/v1/trash", json!({ "item": id })).await.0, 409, "a waiting task went to the trash");
    assert_eq!(alice.task(&id).await["task_id"], json!(id), "the task left the view");

    assert!(alice.post(&format!("/v1/tasks/{id}/cancel"), json!({})).await.0.is_success());
    alice.wait_settled(&id).await;
    assert_eq!(alice.post("/v1/trash", json!({ "item": id })).await.0, 204, "a cancelled task could not be deleted");
    assert_eq!(mail.effects("send"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_after_a_side_effect_does_not_repeat_it() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![plan("send the report"), send(), complete("sent")])).await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());

    let hold = mail.hold("send");
    let id = alice.submit("Email bob the report.").await;
    tokio::time::timeout(PATIENCE, hold.received()).await.expect("the send reached the app");
    app.kill().await; // the effect happened; its response is lost
    hold.release();
    app.restart().await;

    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(mail.effects("send"), 1, "the email was sent more than once");
    let keys: BTreeSet<_> = mail.calls_to("send").iter().map(|c| c.idempotency_key().map(String::from)).collect();
    assert_eq!(keys.len(), 1, "a resend used a different idempotency key: {keys:?}");
    assert!(keys.iter().next().unwrap().is_some(), "the send carried no idempotency key");
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_against_an_app_without_idempotency_asks_instead_of_resending() {
    let mail = mcp::mail().honor_idempotency(false).start().await;
    let model = FakeModel::start(steps(vec![plan("send the report"), send(), complete("sent")])).await;
    let mut app =
        Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]).not_idempotent("mail"), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());

    let hold = mail.hold("send");
    let id = alice.submit("Email bob the report.").await;
    tokio::time::timeout(PATIENCE, hold.received()).await.expect("the send reached the app");
    app.kill().await;
    hold.release();
    app.restart().await;

    let me = alice.clone();
    let tid = id.clone();
    eventually("an inbox item asks about the unknown outcome", PATIENCE, || {
        let (me, tid) = (me.clone(), tid.clone());
        async move { me.inbox().await.into_iter().find(|i| i["task_id"] == tid.as_str() && i["status"] == "open") }
    })
    .await;
    never("the send was repeated", Duration::from_secs(5), || async { mail.calls_to("send").len() > 1 }).await;
}

fn snapshot_tasks(v: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        v["tasks"].as_array().unwrap().iter().map(|t| (t["task_id"].as_str().unwrap().to_string(), t["status"].as_str().unwrap().to_string())).collect();
    out.sort();
    out
}

fn snapshot_inbox(items: &[Value]) -> Vec<(String, String, String, String)> {
    let mut out: Vec<_> = items
        .iter()
        .map(|i| {
            let s = |k: &str| i[k].as_str().unwrap_or_default().to_string();
            (s("id"), s("kind"), s("task_id"), s("status"))
        })
        .collect();
    out.sort();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn all_state_is_rebuilt_from_the_event_log_and_its_content() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![plan("send the report"), send(), complete("sent")])).await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &model).await;
    let alice = app.api("alice");
    // No permit for send: the run waits for approval.
    let id = alice.submit("Email bob the report.").await;
    alice.wait_item(&id, "approval").await;

    let tasks_before = snapshot_tasks(&alice.tasks().await);
    let inbox_before = snapshot_inbox(&alice.inbox().await);

    app.kill().await;
    let admin = app.db.admin().await;
    for (s, t) in app.db.tables().await {
        if !(s == "public" && (t == "events" || t == "content")) {
            admin.batch_execute(&format!("TRUNCATE \"{s}\".\"{t}\" CASCADE")).await.unwrap();
        }
    }
    app.restart().await;

    assert_eq!(snapshot_tasks(&alice.tasks().await), tasks_before, "task state did not survive on the log and its content");
    assert_eq!(snapshot_inbox(&alice.inbox().await), inbox_before, "inbox did not survive on the log and its content");
    assert_eq!(mail.effects("send"), 0);

    let item = alice.wait_item(&id, "approval").await;
    let st = alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await;
    assert!(st.is_success(), "approve: {st}");
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(mail.effects("send"), 1);
}

/// Every crash lands while a post is in flight: its effect has happened and
/// its response is lost. The resend after each restart must be recognized.
#[tokio::test(flavor = "multi_thread")]
async fn repeated_crashes_never_repeat_an_effect() {
    let chat = mcp::chat().start().await;
    let model = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("post four updates"),
            n @ 1..=4 => tool("chat__post", json!({ "channel": "team", "text": format!("update {n}") })),
            _ => complete("posted"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("chat", &chat, "internal", "private", &[("post", "irreversible"), ("list_posts", "read")]), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("chat__post"))).await.is_success());

    // A held call is one that took effect; the app answers a recognized
    // resend without holding it, so each hold catches the next new post.
    let mut hold = chat.hold("post");
    let id = alice.submit("Post four updates to the team channel.").await;
    for n in 1..=4 {
        tokio::time::timeout(PATIENCE, hold.received()).await.unwrap_or_else(|_| panic!("post {n} never reached the app"));
        app.kill().await;
        let next = (n < 4).then(|| chat.hold("post"));
        hold.release();
        app.restart().await;
        if let Some(h) = next {
            hold = h;
        }
    }
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(app.starts(), 5);
    let mut keys: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for c in chat.calls_to("post") {
        keys.entry(c.args["text"].as_str().unwrap().to_string()).or_default().push(c.idempotency_key().map(String::from));
    }
    for n in 1..=4 {
        let text = format!("update {n}");
        let k = chat.world()["posts"].as_array().unwrap().iter().filter(|p| p["text"] == text.as_str()).count();
        assert_eq!(k, 1, "{text} was posted {k} times across four crashes");
        let sent = &keys[&text];
        assert!(sent.len() >= 2, "{text} was not resent after its crash: {sent:?}");
        assert!(sent.iter().all(|k| k.is_some() && *k == sent[0]), "the resends of {text} did not carry one idempotency key: {sent:?}");
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// An installer that leaves a program behind, so a replay can be seen working.
const INSTALLER: &str = "#!/bin/sh\nprintf '#!/bin/sh\\necho hello-from-the-app\\n' > /usr/local/bin/hello\nchmod 755 /usr/local/bin/hello\n";

fn sha256_of(data: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("sha256sum").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("run sha256sum");
    child.stdin.take().expect("its stdin").write_all(data).expect("hash");
    let out = child.wait_with_output().expect("hash");
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().expect("a hash").to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn destroying_a_computer_loses_no_user_work() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let registry = FakeWeb::start(&[("/hello", INSTALLER)]).await;
    let bare = registry.base.trim_start_matches("http://").to_string();
    let spec = Spec::default().computer_options(json!({ "registries": ["registry.test"], "public_hosts": { "registry.test": [bare] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    let at = |rest: &str| format!("/v1/computers/{id}{rest}");
    let (code, started) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");

    let work = canary("user-work");
    let (code, wrote) = alice.put(&at("/files"), json!({ "path": "/workspace/work", "data_b64": b64(work.as_bytes()) })).await;
    assert_eq!(code, 200, "{wrote}");
    computerd.root_shell(&id, "curl -s -m 20 -o /workspace/installer -x http://10.200.0.1:3128 http://registry.test/hello");
    computerd.exec(&id, "sh /workspace/installer");
    assert!(computerd.exec(&id, "hello").contains("hello-from-the-app"), "the test's own install did not work");
    // The proxy checks what the entry claims against what it carried.
    let untraced = canary("not-downloaded");
    assert_eq!(alice.put(&at("/files"), json!({ "path": "/workspace/made-here", "data_b64": b64(untraced.as_bytes()) })).await.0, 200);
    let claim = |installer: &str, source: &str| json!({ "name": "x", "version": "1", "source": source, "installer": installer, "steps": ["true"] });
    let (code, refused) = alice.post(&at("/apps"), claim("/workspace/made-here", "http://registry.test/hello")).await;
    assert_eq!(code, 400, "a file the proxy never carried was accepted: {refused}");
    let (code, refused) = alice.post(&at("/apps"), claim("/workspace/installer", "http://other.test/hello")).await;
    assert_eq!(code, 400, "a file was accepted from a source it did not come from: {refused}");
    let (code, recorded) = alice
        .post(&at("/apps"), json!({ "name": "hello", "version": "1", "source": "http://registry.test/hello", "installer": "/workspace/installer", "steps": ["sh \"$INSTALLER\""] }))
        .await;
    assert_eq!(code, 200, "{recorded}");
    let hash = sha256_of(INSTALLER.as_bytes());
    assert_eq!(recorded["sha256"], hash.as_str(), "the manifest does not hold the installer's own hash");
    let (code, backed) = alice.post(&at("/backup"), json!({})).await;
    assert_eq!(code, 200, "{backed}");

    let (code, _) = alice.post(&at("/kill"), json!({ "mode": "destroy" })).await;
    assert_eq!(code, 200);
    computerd.remove_overlay_and_snapshot(&id);
    let (code, woke) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{woke}");
    assert_eq!(woke["replayed"], true, "the overlay was new and the manifest was not replayed: {woke}");
    let (code, read) = alice.get(&at("/files?path=/workspace/work")).await;
    assert_eq!((code.as_u16(), read["data_b64"].as_str()), (200, Some(b64(work.as_bytes()).as_str())), "the rebuilt computer lost the file: {read}");
    assert!(computerd.exec(&id, "hello").contains("hello-from-the-app"), "the app was not replayed from the manifest");
    let (_, seen) = alice.get(&at("/apps")).await;
    assert_eq!(seen["apps"][0]["sha256"], hash.as_str(), "the manifest changed: {seen}");
    assert!(seen["replay_error"].is_null(), "{seen}");

    // An installer that is no longer what the manifest says is not run.
    let (code, _) = alice.post(&at("/kill"), json!({ "mode": "destroy" })).await;
    assert_eq!(code, 200);
    computerd.remove_overlay_and_snapshot(&id);
    let cached = app.state_dir.join("computer-installers").join(&hash);
    std::fs::write(&cached, "#!/bin/sh\ntouch /usr/local/bin/tampered\n").unwrap();
    let (code, woke) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{woke}");
    let (_, seen) = alice.get(&at("/apps")).await;
    assert!(seen["replay_error"].as_str().is_some_and(|e| e.contains("does not match")), "a changed installer was accepted: {seen}");
    assert!(!computerd.exec(&id, "ls /usr/local/bin").contains("tampered"), "a changed installer ran");

    // The volume itself comes back from its backup.
    let (code, _) = alice.post(&at("/kill"), json!({ "mode": "destroy" })).await;
    assert_eq!(code, 200);
    computerd.remove_volume(&id);
    assert!(!computerd.volume_exists(&id));
    let (code, restored) = alice.post(&at("/restore"), json!({})).await;
    assert_eq!(code, 200, "the backup did not restore the volume: {restored}");
    std::fs::write(&cached, INSTALLER).unwrap();
    let (code, woke) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{woke}");
    let (code, read) = alice.get(&at("/files?path=/workspace/work")).await;
    assert_eq!((code.as_u16(), read["data_b64"].as_str()), (200, Some(b64(work.as_bytes()).as_str())), "the backup did not restore the volume: {read}");
}

/// A volume's blocks go back to the host after zeroing: well under the
/// 1 GiB test volume, whose free space was all written.
const ZEROED_VOLUME_MAX_BYTES: u64 = 256 << 20;

/// An earlier version of a deleted file lives on in free blocks until they
/// are overwritten, which happens before the computer hibernates.
#[tokio::test(flavor = "multi_thread")]
async fn earlier_versions_of_a_deleted_file_leave_the_volume_when_it_goes_idle() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    let at = |rest: &str| format!("/v1/computers/{id}{rest}");
    assert_eq!(alice.post(&at("/start"), json!({})).await.0, 200);
    let old = canary("old-version");
    computerd.exec(&id, &format!("echo {old} > /workspace/report; sync; echo newer > /workspace/report.new; mv /workspace/report.new /workspace/report; sync"));
    assert_eq!(alice.delete(&at("/files?path=/workspace/report")).await.0, 200);
    computerd.exec(&id, "sync");
    let (code, slept) = alice.post(&at("/hibernate"), json!({})).await;
    assert_eq!(code, 200, "{slept}");
    assert!(!computerd.volume_holds(&id, &old), "an earlier version of a deleted file stayed in the volume's free blocks");
    let cost = computerd.volume_allocated_bytes(&id);
    assert!(cost < ZEROED_VOLUME_MAX_BYTES, "zeroing left the host holding {cost} bytes of nothing");
}

/// The host's unflushed pages count against a VM's memory limit, so a big
/// write used to get the VM killed.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_survives_writing_far_more_than_its_memory() {
    let computerd = Computerd::start();
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{id}/start"), json!({})).await.0, 200);
    let done = computerd.exec(&id, "for i in 1 2 3 4; do dd if=/dev/zero of=/workspace/big bs=1M count=900 2>/dev/null; rm /workspace/big; done; sync; echo still-here");
    assert!(done.contains("still-here"), "{done}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_file_is_gone_at_once_and_nothing_outlives_the_purge_window() {
    let computerd = Computerd::start();
    assert!(computerd.purge_days() <= 30, "the purge window is over 30 days");
    let systems = Arc::new(std::sync::Mutex::new(String::new()));
    let seen_by_model = systems.clone();
    let model = FakeModel::start(script(move |c| {
        seen_by_model.lock().unwrap().push_str(&c.body["system"].to_string());
        async move {
            match c.turn() {
                0 => plan("nothing"),
                _ => fail("done"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    let at = |rest: &str| format!("/v1/computers/{id}{rest}");
    let (code, started) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");

    let secret = canary("deleted-file");
    let (code, wrote) = alice.put(&at("/files"), json!({ "path": "/workspace/secret.txt", "data_b64": b64(secret.as_bytes()) })).await;
    assert_eq!(code, 200, "{wrote}");
    let (code, before) = alice.post(&at("/backup"), json!({})).await;
    assert_eq!(code, 200, "{before}");
    assert!(computerd.backups_hold(&id, &secret), "the scan found nothing it should have");
    // Backups are named by the second, and this one must be the older.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Hibernated, so a snapshot from before the deletion exists too.
    let (code, _) = alice.post(&at("/hibernate"), json!({})).await;
    assert_eq!(code, 200);
    assert!(computerd.has_snapshot(&id));

    let (code, gone) = alice.send(reqwest::Method::DELETE, &at("/files?path=/workspace/secret.txt"), None).await;
    assert_eq!(code, 200, "{gone}");
    assert!(!computerd.volume_holds(&id, &secret), "the deleted file is still stored: on the volume");
    assert!(!computerd.has_snapshot(&id), "a snapshot outlived the edit it no longer matches");
    let (code, read) = alice.get(&at("/files?path=/workspace/secret.txt")).await;
    assert!(code != 200, "the deleted file can still be read: {read}");

    // The log keeps that it happened and none of what it was.
    let task = alice.submit("Do nothing.").await;
    alice.wait_settled(&task).await;
    assert!(systems.lock().unwrap().contains("/workspace/secret.txt"), "the next run was not told");
    assert!(magnus_invariants::scan::files_containing(&app.state_dir, &secret).is_empty(), "the deleted file is still stored: in the server's state");

    let (code, started) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");
    let (code, after) = alice.post(&at("/backup"), json!({})).await;
    assert_eq!(code, 200, "{after}");

    // One second before the newer backup expires: the older one must be gone.
    let window_seconds = computerd.purge_days() * 86_400;
    let newest = *computerd.backups(&id).last().unwrap();
    assert_eq!(computerd.backups(&id).len(), 2, "{:?}", computerd.backups(&id));
    computerd.purge_at(newest + window_seconds - 1);
    assert_eq!(computerd.backups(&id), vec![newest], "a backup outlived the purge window");
    assert!(!computerd.backups_hold(&id, &secret), "the deleted file is still stored: in a backup");
    computerd.purge_at(newest + window_seconds + 1);
    assert!(computerd.backups(&id).is_empty(), "a backup outlived the purge window");
}
