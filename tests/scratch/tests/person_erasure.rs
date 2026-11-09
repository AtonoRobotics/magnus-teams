//! A person deleted is gone for good (admin console spec, Users; connectors
//! spec, Isolation), against a real Keycloak: nothing the appliance kept for
//! them outlives them, so whoever is later known by their name starts with
//! nothing of theirs, and nobody else's sign-ins go with them.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

async fn roles(alice: &Api, who: &str) -> Value {
    let (_, v) = alice.get("/v1/people").await;
    v["people"].as_array().unwrap().iter().find(|p| p["id"] == who).cloned().unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_deleted_keeps_no_roles_and_takes_only_their_own_sign_ins() {
    let m = FakeModel::start(steps(vec![])).await;
    // Someone whose name ends the way a sign-in of Bob's does.
    let mut spec = Spec::default();
    let mut ops = spec.people.iter().find(|p| p.id == "carol").unwrap().clone();
    (ops.id, ops.name, ops.token) = ("ops__bob".into(), "ops__bob".into(), format!("tok-ops-{}", std::process::id()));
    spec.people.push(ops);
    let mut app = Appliance::start(spec, &m).await;
    let kc = Keycloak::start().await;
    kc.add_person("bob").await;
    kc.configure(&mut app).await;
    let (st, v) = kc.app_sign_in(&app, "bob").await;
    assert_eq!(st, 200, "{v}");
    let mut bob = app.api("bob");
    bob.token = v["token"].as_str().unwrap().to_string();
    let (alice, ops) = (app.api("alice"), app.api("ops__bob"));

    // The owner makes Bob an admin; Bob and ops__bob each sign in to an app.
    assert_eq!(alice.put("/v1/people/bob/roles", json!({ "roles": ["admin"] })).await.0.as_u16(), 204);
    assert_eq!(roles(&alice, "bob").await["roles"], json!(["admin"]));
    // Bob also signs in to an app whose name ends in `_`, as names may.
    let (cal, notes) = (FakeOAuthApp::start(3600).await, FakeOAuthApp::start(3600).await);
    for (name, url) in [("calendar", &cal.url), ("notes_", &notes.url)] {
        let (st, v) = alice.post("/v1/connectors", json!({ "name": name, "url": url, "audience": "private", "label": "private" })).await;
        assert!(st.is_success(), "{v}");
    }
    for (api, name) in [(&bob, "calendar"), (&ops, "calendar"), (&bob, "notes_")] {
        let (st, v) = api.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
        assert!(st.is_success(), "{v}");
        let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
        assert_eq!(api.post(&format!("/v1/connectors/{name}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
    }

    // Root deletes Bob, as the console names him: by his Keycloak account.
    let (_, people) = control(&app, "GET", "/v1/people", Value::Null).await;
    let sub = people["people"].as_object().unwrap().keys().next().expect("bob's account is bound").clone();
    let (st, v) = control(&app, "POST", &format!("/v1/people/{sub}/erase"), json!({ "actor": "admin:root" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["sign_ins_revoked"], 2, "bob's two sign-ins, and only his: {v}");
    assert!(!notes.state.lock().unwrap().revoked.is_empty(), "bob's sign-in to notes_ was not revoked at the app");

    // Bob keeps no role the owner gave him: whoever is next known as Bob
    // starts from the organisation's config, not as an admin.
    let after = roles(&alice, "bob").await;
    assert_eq!(after["roles_from"], "config", "{after}");
    assert!(!after["roles"].as_array().unwrap().iter().any(|r| r == "admin"), "bob is still an admin after he was deleted: {after}");
    // ops__bob's sign-in was not Bob's.
    let (_, c) = ops.get("/v1/connectors/calendar").await;
    assert_eq!(c["state"], "connected", "deleting bob ended ops__bob's sign-in: {c}");
}

/// Bob as he signs in at the realm now: the account Keycloak has for him.
async fn signed_in(kc: &Keycloak, app: &Appliance) -> Api {
    let (st, v) = kc.app_sign_in(app, "bob").await;
    assert_eq!(st, 200, "{v}");
    let mut bob = app.api("bob");
    bob.token = v["token"].as_str().unwrap().to_string();
    bob
}

/// Whether the task's update is asked about (true) or runs (false).
async fn asked(api: &Api, t: &str, crm: &FakeMcp, before: usize) -> bool {
    eventually("the update is asked about or runs", PATIENCE, || {
        let (api, t) = (api.clone(), t.to_string());
        async move {
            if crm.effects("update_record") > before {
                return Some(false);
            }
            (!api.open_items(&t, "approval").await.is_empty()).then_some(true)
        }
    })
    .await
}

/// A person deleted takes the approvals given to them (#191): the owner's
/// standing approval of Bob's update covers nobody later known as Bob, a
/// new Keycloak account bound to his name, neither live nor once the state
/// is rebuilt from the log. What Bob approved for Carol stays hers: it was
/// valid when given and is about her work.
#[tokio::test(flavor = "multi_thread")]
async fn a_person_deleted_takes_the_approvals_given_to_them_but_not_those_they_gave() {
    let crm = mcp::crm().start().await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("close the record", "the record is closed"),
            1 => tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } })),
            _ => complete("closed"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("update_record", "irreversible")]), &m).await;
    let kc = Keycloak::start().await;
    kc.add_person("bob").await;
    kc.configure(&mut app).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let bob = signed_in(&kc, &app).await;

    // The owner approves Bob's update for good; Bob approves Carol's for an hour.
    let t = bob.submit("Close record 42.").await;
    let item = bob.wait_item(&t, "approval").await;
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "standing" })).await.is_success());
    bob.wait_settled(&t).await;
    assert_eq!(crm.effects("update_record"), 1);
    let t = carol.submit("Close record 42.").await;
    let item = carol.wait_item(&t, "approval").await;
    assert!(bob.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "window", "minutes": 60 })).await.is_success());
    carol.wait_settled(&t).await;
    assert_eq!(crm.effects("update_record"), 2);

    // Root deletes Bob.
    let (_, people) = control(&app, "GET", "/v1/people", Value::Null).await;
    let old = people["people"].as_object().unwrap().keys().next().expect("bob's account is bound").clone();
    let (st, v) = control(&app, "POST", &format!("/v1/people/{old}/erase"), json!({ "actor": "admin:root" })).await;
    assert_eq!(st, 200, "{v}");

    // What Bob approved for Carol still covers her update.
    let t = carol.submit("Close record 42 again.").await;
    assert!(!asked(&carol, &t, &crm, 2).await, "deleting bob took back the approval he gave carol");
    carol.wait_settled(&t).await;

    // Bob's account is made again: a new account, bound to his name.
    kc.delete("bob").await;
    kc.add_person("bob").await;
    let bob = signed_in(&kc, &app).await;
    let (_, people) = control(&app, "GET", "/v1/people", Value::Null).await;
    assert!(!people["people"].as_object().unwrap().contains_key(&old), "the old account is still bound: {people}");
    let n = crm.effects("update_record");
    let t = bob.submit("Close record 42 again.").await;
    assert!(asked(&bob, &t, &crm, n).await, "an approval given to the deleted bob covered the new account known as bob");

    // And once the state is rebuilt from the log.
    app.restart().await;
    let bob = signed_in(&kc, &app).await;
    let t = bob.submit("Close record 42 once more.").await;
    assert!(asked(&bob, &t, &crm, n).await, "after a restart, an approval given to the deleted bob covered the new bob");
    assert_eq!(crm.effects("update_record"), n);
}

/// The backups directory read-only for as long as it is held, as a full or
/// read-only disk would be: a backup in it cannot be rewritten. Writable
/// again when dropped, so the test's directory can be cleaned up.
struct ReadOnly(std::path::PathBuf);

impl ReadOnly {
    fn new(dir: &std::path::Path) -> ReadOnly {
        let held = ReadOnly::mode(dir, 0o555);
        assert!(std::fs::write(dir.join("probe"), b"").is_err(), "{} is still writable (as root?): the test cannot make a backup fail", dir.display());
        held
    }

    /// Not even listed, as a disk that cannot be read.
    fn unreadable(dir: &std::path::Path) -> ReadOnly {
        let held = ReadOnly::mode(dir, 0o000);
        assert!(std::fs::read_dir(dir).is_err(), "{} can still be listed (as root?): the test cannot make it unreadable", dir.display());
        held
    }

    fn mode(dir: &std::path::Path, mode: u32) -> ReadOnly {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        ReadOnly(dir.to_path_buf())
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

const BOB_WORDS: &str = "BOB-ERASED-CANARY-195";

/// Bob, with a task of his in the store and in a backup the owner made,
/// then deleted by root while that backup cannot be rewritten: the backup's
/// path, Bob's subject, the erasure's answer, and the hold on the directory.
async fn erased_while_a_backup_is_stuck(app: &mut Appliance, kc: &Keycloak) -> (std::path::PathBuf, String, Value, ReadOnly) {
    kc.add_person("bob").await;
    kc.configure(app).await;
    let bob = signed_in(kc, app).await;
    let t = bob.submit(&format!("Write about {BOB_WORDS}.")).await;
    bob.wait_settled(&t).await;
    let (st, b) = app.api("alice").post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    let dir = app.state_dir.join("backups");
    let file = dir.join(b["file"].as_str().unwrap());
    let stuck = ReadOnly::new(&dir);
    let (_, people) = control(app, "GET", "/v1/people", Value::Null).await;
    let sub = people["people"].as_object().unwrap().keys().next().expect("bob's account is bound").clone();
    let (st, v) = control(app, "POST", &format!("/v1/people/{sub}/erase"), json!({ "actor": "admin:root" })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["content_removed"].as_u64().unwrap_or(0) > 0, "bob had no content to erase: {v}");
    assert!(!app.db.dump().await.contains(BOB_WORDS), "bob's words are still in the store");
    (file, sub, v, stuck)
}

fn writer() -> magnus_invariants::model::Script {
    script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("write it", "it is written"),
            _ => complete("written"),
        }
    })
}

async fn audited(app: &Appliance, action: &str) -> Vec<Value> {
    let (_, a) = control(app, "GET", &format!("/v1/audit?action={action}"), Value::Null).await;
    a["entries"].as_array().cloned().unwrap_or_default()
}

/// A person deleted while a backup cannot be rewritten is not reported
/// erased (#195): the answer, the owner's Backups and the audit say their
/// data is still in that backup; after a restart that is still so and it is
/// tried again, and once the backup can be rewritten, it is, and the
/// erasure is reported complete.
#[tokio::test(flavor = "multi_thread")]
async fn an_erasure_a_backup_cannot_take_stays_incomplete_until_the_backup_is_rewritten() {
    let m = FakeModel::start(writer()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    let (file, sub, v, stuck) = erased_while_a_backup_is_stuck(&mut app, &kc).await;
    let name = file.file_name().unwrap().to_str().unwrap().to_string();
    let before = std::fs::read(&file).unwrap();

    // Reported incomplete: in the answer, the owner's Backups and the audit.
    assert_eq!(v["erasure"], "incomplete", "an erasure whose backup could not be rewritten was reported complete: {v}");
    assert_eq!(v["backups_pending"], json!([name]), "{v}");
    let alice = app.api("alice");
    let (_, b) = alice.get("/v1/backups").await;
    assert_eq!(b["scrub_pending"]["files"], json!([name]), "{b}");
    assert_eq!(b["scrub_pending"]["erasures"], 1, "{b}");
    let entries = audited(&app, "user_erasure_incomplete").await;
    assert_eq!(entries.first().map(|e| e["target"].clone()), Some(json!(sub)), "{entries:?}");
    assert!(audited(&app, "user_erasure_completed").await.is_empty());

    // After a restart, still pending: it is in the log, and tried again.
    app.restart().await;
    let (_, b) = alice.get("/v1/backups").await;
    assert_eq!(b["scrub_pending"]["files"], json!([name]), "the pending cleanup did not survive a restart: {b}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "a backup in a read-only directory changed");

    // The backup can be written again: the next try rewrites it, and the
    // erasure is complete.
    drop(stuck);
    app.restart().await;
    eventually("the backup is rewritten after the restart", PATIENCE, || {
        let alice = alice.clone();
        async move { alice.get("/v1/backups").await.1["scrub_pending"].is_null().then_some(()) }
    })
    .await;
    assert_ne!(std::fs::read(&file).unwrap(), before, "the backup was not rewritten");
    let entries = audited(&app, "user_erasure_completed").await;
    assert_eq!(entries.first().map(|e| e["target"].clone()), Some(json!(sub)), "{entries:?}");
}

/// A restore from a backup that could not be rewritten after a person was
/// deleted does not bring their content back (#195).
#[tokio::test(flavor = "multi_thread")]
async fn a_backup_left_holding_an_erased_persons_content_does_not_restore_it() {
    let m = FakeModel::start(writer()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    let (file, _, _, _stuck) = erased_while_a_backup_is_stuck(&mut app, &kc).await;

    // The disaster: the log, its content and the vault are gone; the backup
    // that still holds Bob's words is restored.
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    let out = std::process::Command::new(magnus_invariants::appliance::bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!app.db.dump().await.contains(BOB_WORDS), "bob's words came back from a backup the erasure could not rewrite");
    app.restart().await;
}

/// A task of Alice's, deleted from the trash: a removal, and the scrub of
/// the backups it runs before it answers.
async fn remove_a_task(alice: &Api, words: &str) {
    let t = alice.submit(words).await;
    let review = alice.wait_item(&t, "review").await;
    assert_eq!(alice.respond(review["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.as_u16(), 200);
    alice.wait_settled(&t).await;
    let (st, v) = alice.post("/v1/trash", json!({ "item": t })).await;
    assert!(st.is_success(), "{v}");
    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{t}"), None).await;
    assert!(st.is_success(), "{v}");
}

/// The disaster: the log, its content and the vault are gone, and `file`
/// is restored into empty ones.
async fn restore_from(app: &mut Appliance, file: &std::path::Path) {
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    let out = std::process::Command::new(magnus_invariants::appliance::bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore of {} failed: {}", file.display(), String::from_utf8_lossy(&out.stderr));
}

/// A backups directory gone (a disk not mounted) or unreadable while an
/// erasure waits on a backup in it is not one without backups: the backup
/// is out of reach, not rewritten, so the erasure stays incomplete and the
/// scrub pending (#243 review).
#[tokio::test(flavor = "multi_thread")]
async fn an_erasure_stays_incomplete_while_the_backups_directory_is_out_of_reach() {
    let m = FakeModel::start(writer()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    let (file, _, v, stuck) = erased_while_a_backup_is_stuck(&mut app, &kc).await;
    assert_eq!(v["erasure"], "incomplete", "{v}");
    let name = file.file_name().unwrap().to_str().unwrap().to_string();
    let dir = file.parent().unwrap().to_path_buf();
    drop(stuck);
    let alice = app.api("alice");

    // Gone: the next scrub, after a removal, finds no directory.
    let away = dir.with_file_name("backups-away");
    std::fs::rename(&dir, &away).unwrap();
    remove_a_task(&alice, "Write about the gone directory.").await;
    let (_, b) = alice.get("/v1/backups").await;
    assert_eq!(b["scrub_pending"]["files"], json!([name]), "a missing backups directory was taken for one without backups: {b}");

    // Back, but unreadable: the scrub cannot list it.
    std::fs::rename(&away, &dir).unwrap();
    let hidden = ReadOnly::unreadable(&dir);
    remove_a_task(&alice, "Write about the unreadable directory.").await;
    let (_, b) = alice.get("/v1/backups").await;
    assert_eq!(b["scrub_pending"]["files"], json!([name]), "an unreadable backups directory was taken for one without backups: {b}");
    assert!(audited(&app, "user_erasure_completed").await.is_empty(), "the erasure was reported complete while its backup was out of reach");
    drop(hidden);
}

fn modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A restore of a backup older than an erasure, whose log does not have
/// it, and a scrub after it do not forget the erasure: a later restore of a
/// backup the erasure could not rewrite still leaves the person's content
/// out (#243 review).
#[tokio::test(flavor = "multi_thread")]
async fn a_restore_of_an_older_backup_does_not_forget_a_later_erasure() {
    let m = FakeModel::start(writer()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    let alice = app.api("alice");
    // An earlier removal, so the log restored has one, then the older
    // backup (Y).
    remove_a_task(&alice, "Write about an earlier removal.").await;
    let (st, y) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{y}");
    let older = app.state_dir.join("backups").join(y["file"].as_str().unwrap());
    // Bob's words in a later backup (X) that his erasure cannot rewrite.
    let (later, _, v, stuck) = erased_while_a_backup_is_stuck(&mut app, &kc).await;
    assert_eq!(v["erasure"], "incomplete", "{v}");

    // Y restored; the backups writable again, and a scrub runs at start.
    restore_from(&mut app, &older).await;
    drop(stuck);
    let record = app.state_dir.join("backups").join("removals.json");
    let was = modified(&record);
    app.restart().await;
    eventually("a scrub after the restore records the removals", PATIENCE, || {
        let record = record.clone();
        async move { (modified(&record) != was).then_some(()) }
    })
    .await;

    // Then X restored: Bob's words stay out.
    restore_from(&mut app, &later).await;
    assert!(!app.db.dump().await.contains(BOB_WORDS), "bob's words came back from a backup his erasure could not rewrite, after a restore of an older one");
    app.restart().await;
}
