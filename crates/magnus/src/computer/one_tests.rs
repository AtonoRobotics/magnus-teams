//! One computer, always there: against a kernel booted on a fresh database
//! and a computerd that answers at once and remembers what it was asked.

use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

type Asked = Arc<Mutex<Vec<Value>>>;

/// Creates, wakes, destroys and erases at once; its guests answer every
/// desktop action with a screenshot. As the real one, a computer whose
/// overlay was thrown away (a destroy, an erase) wakes on a fresh one.
fn fake_computerd(socket: &Path) -> Asked {
    let asked: Asked = Arc::default();
    let listener = UnixListener::bind(socket).unwrap();
    let log = asked.clone();
    let fresh: Arc<Mutex<std::collections::HashSet<String>>> = Arc::default();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            let log = log.clone();
            let fresh = fresh.clone();
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let req: Value = serde_json::from_str(&line).unwrap();
                    log.lock().unwrap().push(req.clone());
                    let id = req["id"].as_str().unwrap_or_default().to_string();
                    let reply = match req["op"].as_str().unwrap() {
                        "wake" => {
                            let new = fresh.lock().unwrap().remove(&id);
                            json!({ "ok": true, "resumed": !new, "overlay_fresh": new })
                        }
                        "destroy" | "erase_volume" => {
                            fresh.lock().unwrap().insert(id);
                            json!({ "ok": true })
                        }
                        "write" => json!({}),
                        "create" | "connect" | "ping" | "hibernate" => json!({ "ok": true }),
                        "exec" => json!({ "exit_code": 0 }),
                        "read" => json!({ "data_b64": "SHOT" }),
                        other => json!({ "ok": false, "error": format!("unexpected {other}") }),
                    };
                    w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                }
            });
        }
    });
    asked
}

fn pg() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

struct Server {
    k: Arc<Kernel>,
    asked: Asked,
    db: String,
    _dir: tempfile::TempDir,
}

/// sam owns the server, ana is an admin and mo a member; the server has
/// computers.
async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let name = format!("one_{}", uuid::Uuid::new_v4().simple());
    let admin = crate::log::connect(&pg()).await.unwrap();
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
    let mut u = url::Url::parse(&pg()).unwrap();
    u.set_path(&format!("/{name}"));
    let url = u.to_string();
    let c = crate::log::connect(&url).await.unwrap();
    c.batch_execute(
        "CREATE TABLE public.events (seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, at timestamptz NOT NULL DEFAULT now(), kind text NOT NULL, actor text NOT NULL, run_id text, body jsonb NOT NULL);
         CREATE TABLE public.content (seq bigint PRIMARY KEY REFERENCES public.events (seq), salt bytea NOT NULL, body jsonb NOT NULL);",
    )
    .await
    .unwrap();
    c.batch_execute(&crate::accounts::migrate_sql("postgres")).await.unwrap();
    let socket = dir.path().join("computerd.sock");
    let asked = fake_computerd(&socket);
    let p = dir.path().join("magnus.toml");
    let state = dir.path().join("state");
    std::fs::write(
        &p,
        format!(
            "[appliance]\nstate_dir = {state:?}\nlisten = \"127.0.0.1:0\"\n[database]\nurl = {url:?}\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n[computer]\nsocket = {socket:?}\n\
             [[people]]\nid = \"sam\"\nname = \"Sam\"\nroles = [\"owner\"]\n[[people]]\nid = \"ana\"\nname = \"Ana\"\nroles = [\"admin\"]\n[[people]]\nid = \"mo\"\nname = \"Mo\"\n"
        ),
    )
    .unwrap();
    let k = Kernel::boot(crate::config::Config::load(&p).unwrap()).await.unwrap();
    Server { k, asked, db: name, _dir: dir }
}

impl Server {
    async fn done(self) {
        let admin = crate::log::connect(&pg()).await.unwrap();
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.db)).await.unwrap();
    }

    fn computers_of(&self, owner: &Owner) -> Vec<Computer> {
        self.k.read(|s| s.computers.all().filter(|c| c.role == Role::Computer && c.owner == *owner).cloned().collect())
    }

    fn asked(&self, op: &str) -> Vec<Value> {
        self.asked.lock().unwrap().iter().filter(|r| r["op"] == op).cloned().collect()
    }

    /// A computer made the way it was before rev 142: by hand, one of several.
    async fn made_by_hand(&self, owner: &Owner) -> String {
        let id = super::super::service::new_id();
        self.k.emit("person:sam", Ev::Computer(Change::Created { id: id.clone(), owner: owner.clone() })).await.unwrap();
        id
    }
}

fn person(id: &str) -> Owner {
    Owner::Person(id.into())
}

#[test]
fn the_one_kept_is_the_one_used_last_and_a_paused_one_is_still_the_one() {
    let mut l = Ledger::default();
    let mo = person("mo");
    let t = chrono::Utc::now();
    l.apply_at(&Change::Created { id: "old".into(), owner: mo.clone() }, t);
    l.apply_at(&Change::Created { id: "new".into(), owner: mo.clone() }, t + chrono::Duration::seconds(1));
    assert_eq!(l.one_of(&mo).map(|c| c.id.as_str()), Some("new"));
    l.apply_at(&Change::Hibernated { id: "old".into() }, t + chrono::Duration::seconds(2));
    assert_eq!(l.one_of(&mo).map(|c| c.id.as_str()), Some("old"), "the computer used last was not the one kept");
    // The kill switch's pause holds the agent off its computer; the run is
    // never sent to another one instead.
    l.apply_at(&Change::Killed { id: "old".into(), mode: KillMode::Pause, delete_volume: false }, t + chrono::Duration::seconds(3));
    assert_eq!(l.one_of(&mo).map(|c| c.id.as_str()), Some("old"), "a paused computer's use went elsewhere");
    let crowded = l.crowded();
    assert_eq!(crowded.len(), 1);
    assert_eq!((crowded[0].0.id.as_str(), crowded[0].1.iter().map(|c| c.id.as_str()).collect::<Vec<_>>()), ("old", vec!["new"]));
    assert!(l.one_of(&person("sam")).is_none());
}

#[test]
fn every_change_names_its_computer() {
    let kept = Change::Kept { id: "a".into(), owner: person("mo"), removed: vec!["b".into()] };
    assert_eq!(kept.computer(), "a");
    let text = serde_json::to_value(crate::events::Ev::Computer(kept.clone())).unwrap();
    assert_eq!((text["kind"].as_str(), text["change"].as_str()), (Some("computer"), Some("kept")));
    let back: crate::events::Ev = serde_json::from_value(text).unwrap();
    assert!(matches!(back, crate::events::Ev::Computer(Change::Kept { .. })));
}

/// Each person and workspace gets its computer with nobody making one, and
/// the look on every start makes none twice.
#[tokio::test(flavor = "multi_thread")]
async fn everyone_gets_one_computer_on_start_and_the_next_start_makes_no_more() {
    let s = server().await;
    let ws = "w1";
    s.k.emit("person:sam", Ev::Team(crate::team::Change::Created { workspace: ws.into(), name: "Ops".into(), owner: "sam".into() })).await.unwrap();
    s.k.reconcile_computers().await;
    for owner in [person("sam"), person("ana"), person("mo"), Owner::Workspace(ws.into())] {
        assert_eq!(s.computers_of(&owner).len(), 1, "{owner:?} does not have exactly one computer");
    }
    let made = s.asked("create").len();
    assert_eq!(made, 4);
    s.k.reconcile_computers().await;
    assert_eq!(s.asked("create").len(), made, "a second look made more computers");
    let sam = s.k.person("sam").unwrap();
    assert_eq!(s.k.my_computer(&sam, None).await.unwrap()["id"], json!(s.computers_of(&person("sam"))[0].id), "asking for a computer did not give the one there is");
    let mo = s.k.person("mo").unwrap();
    assert!(s.k.my_computer(&mo, Some(ws)).await.is_err(), "a member was given the workspace's computer to look after");
    assert_eq!(s.asked("create").len(), made, "asking for a computer made one");
    s.done().await;
}

/// A tool call on a computer that was never started, one that sleeps, and
/// for someone who has none yet wakes (or makes) it and goes on: there is no
/// "You have no running computer".
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_call_wakes_or_makes_the_computer_and_goes_on() {
    let s = server().await;
    let shot = json!({});
    // Nobody has a computer yet: the first use makes it.
    let done = s.k.use_computer(&person("mo"), "run:r1", "computer_screenshot", &shot).await.unwrap().expect("went to the sandbox");
    assert_eq!((done.0.as_str(), done.1), ("Screenshot of your computer attached.", false), "{done:?}");
    assert_eq!(done.2.unwrap()["image_b64"], "SHOT");
    let mine = s.computers_of(&person("mo"));
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].status, Status::Running, "the use did not wake it");
    // Asleep: woken by the next use.
    s.k.emit("kernel", Ev::Computer(Change::Hibernated { id: mine[0].id.clone() })).await.unwrap();
    let wakes = s.asked("wake").len();
    let done = s.k.use_computer(&person("mo"), "run:r2", "computer_screenshot", &shot).await.unwrap().unwrap();
    assert!(!done.1, "{done:?}");
    assert_eq!(s.asked("wake").len(), wakes + 1, "a sleeping computer was not woken");
    // Made by the look at start and never started.
    s.k.reconcile_computers().await;
    let ana = s.computers_of(&person("ana"));
    assert_eq!(ana[0].status, Status::Stopped);
    let done = s.k.use_computer(&person("ana"), "run:r3", "computer_screenshot", &shot).await.unwrap().unwrap();
    assert!(!done.1, "{done:?}");
    assert_eq!(s.computers_of(&person("ana")).len(), 1, "a use made a second computer");
    s.done().await;
}

/// Destroy became reset, and erasing the files rebuilds the same computer
/// (spec rev 142): its id, owner and apps stay, only its volume is replaced
/// by an empty one, the passwords and tokens kept for it go, and only its
/// own person may erase it.
#[tokio::test(flavor = "multi_thread")]
async fn erasing_the_files_keeps_the_same_computer_with_an_empty_volume() {
    let s = server().await;
    s.k.reconcile_computers().await;
    let id = s.computers_of(&person("mo"))[0].id.clone();
    s.k.emit("kernel", Ev::Computer(Change::AppInstalled { id: id.clone(), app: an_app("0".repeat(64)) })).await.unwrap();
    s.k.vault.put(&format!("surrogate/{id}/github"), "real").unwrap();
    let sam = s.k.person("sam").unwrap();
    assert!(s.k.kill_computer(&sam, &id, KillMode::Destroy, true).await.is_err(), "an admin erased a person's files");
    let mo = s.k.person("mo").unwrap();
    let reset = s.k.kill_computer(&mo, &id, KillMode::Destroy, true).await.unwrap();
    assert_eq!(reset["id"], json!(id));
    let now = s.computers_of(&person("mo"));
    assert_eq!(now.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [id.as_str()], "the erase did not keep the same computer");
    assert_eq!(now[0].apps.len(), 1, "the erase lost the computer's apps");
    assert!(s.asked("erase_volume").iter().any(|r| r["id"] == json!(id)), "the volume was not erased");
    assert!(!s.asked("destroy").iter().any(|r| r["delete_volume"] == true), "the erase deleted the computer");
    assert!(!s.k.vault.names().unwrap().iter().any(|n| n.starts_with(&format!("surrogate/{id}/"))), "the vault kept the erased computer's tokens");
    s.k.reset_computer(&mo, &id, false).await.unwrap();
    assert_eq!(s.asked("erase_volume").len(), 2);
    assert_eq!(s.computers_of(&person("mo")).len(), 1);
    s.done().await;
}

fn an_app(sha256: String) -> super::super::App {
    super::super::App { name: "notes".into(), version: "1".into(), source: "https://example.com".into(), sha256, steps: vec!["sh $INSTALLER".into()], hosts: vec![] }
}

/// Both kinds of reset rebuild from the base image and the manifest: the
/// next use puts the installed app back on the fresh system.
#[tokio::test(flavor = "multi_thread")]
async fn an_installed_app_comes_back_after_either_reset() {
    let s = server().await;
    s.k.reconcile_computers().await;
    let id = s.computers_of(&person("mo"))[0].id.clone();
    let installer = b"#!/bin/sh\necho installed\n";
    let sha = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(installer));
    let dir = s.k.computer_installers();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(&sha), installer).unwrap();
    s.k.emit("kernel", Ev::Computer(Change::AppInstalled { id: id.clone(), app: an_app(sha.clone()) })).await.unwrap();
    let mo = s.k.person("mo").unwrap();
    for keep_files in [true, false] {
        s.k.reset_computer(&mo, &id, keep_files).await.unwrap();
        let before = s.asked("exec").iter().filter(|r| r["command"].as_str().unwrap_or_default().contains("sh $INSTALLER")).count();
        s.k.use_computer(&person("mo"), "run:r1", "computer_screenshot", &json!({})).await.unwrap().unwrap();
        let installed = s.asked("exec").iter().filter(|r| r["command"].as_str().unwrap_or_default().contains("sh $INSTALLER")).count();
        assert_eq!(installed, before + 1, "the app was not installed again after a reset keeping files: {keep_files}");
        assert!(s.asked("write").iter().any(|r| r["path"].as_str().is_some_and(|p| p.ends_with(&sha))), "the installer was not put on the computer");
        assert_eq!(s.computers_of(&person("mo"))[0].id, id);
    }
    s.done().await;
}

/// Removing a person deletes their computer by the usual deletion, and the
/// look at start deletes the computer of anyone the server no longer keeps;
/// a person from the config keeps theirs.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_person_takes_their_computer_with_them() {
    let s = server().await;
    s.k.accounts.add_person("lu", "Lu", None).await.unwrap();
    s.k.accounts.add_person("vi", "Vi", None).await.unwrap();
    s.k.reconcile_computers().await;
    let lu = s.computers_of(&person("lu"))[0].id.clone();
    let vi = s.computers_of(&person("vi"))[0].id.clone();
    s.k.remove_local_person("lu").await.unwrap();
    assert!(s.computers_of(&person("lu")).is_empty(), "the removed person's computer stayed");
    assert!(s.asked("destroy").iter().any(|r| r["id"] == json!(lu) && r["delete_volume"] == true), "their computer was not deleted");
    // Removed while the server was not looking, as the command line does.
    s.k.accounts.remove_person("vi").await.unwrap();
    s.k.reconcile_computers().await;
    assert!(s.computers_of(&person("vi")).is_empty(), "the look did not delete a gone person's computer");
    assert!(s.asked("destroy").iter().any(|r| r["id"] == json!(vi) && r["delete_volume"] == true));
    for p in ["sam", "ana", "mo"] {
        assert_eq!(s.computers_of(&person(p)).len(), 1, "{p} from the config lost their computer");
    }
    s.done().await;
}

/// An owner with several computers from before keeps the one used last,
/// with its volume; the others go by the usual deletion, and the owner is
/// told what was kept and what went. Done once.
#[tokio::test(flavor = "multi_thread")]
async fn several_computers_become_the_one_used_last_and_the_person_is_told() {
    let s = server().await;
    let mo = person("mo");
    let a = s.made_by_hand(&mo).await;
    let b = s.made_by_hand(&mo).await;
    let c = s.made_by_hand(&mo).await;
    s.k.emit("person:mo", Ev::Computer(Change::Started { id: b.clone() })).await.unwrap();
    s.k.reconcile_computers().await;
    let left = s.computers_of(&mo);
    assert_eq!(left.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [b.as_str()], "the computer used last was not the one kept");
    let destroyed: Vec<Value> = s.asked("destroy");
    for gone in [&a, &c] {
        assert!(destroyed.iter().any(|r| r["id"] == json!(gone) && r["delete_volume"] == true), "{gone} was not deleted");
    }
    assert!(!destroyed.iter().any(|r| r["id"] == json!(b)), "the kept computer's volume was touched");
    let notices: Vec<crate::state::Item> = s.k.read(|st| st.items.values().filter(|i| i.person == "mo" && i.details["reason"] == "one_computer").cloned().collect());
    assert_eq!(notices.len(), 1, "the person was not told, or told twice");
    assert!(notices[0].summary.contains("2 other computers were removed") && notices[0].summary.contains("30 days"), "{}", notices[0].summary);
    assert_eq!(notices[0].details["kept"], json!(b));
    s.k.reconcile_computers().await;
    assert_eq!(s.asked("destroy").len(), destroyed.len(), "the next start deleted again");
    let told = s.k.read(|st| st.items.values().filter(|i| i.details["reason"] == "one_computer").count());
    assert_eq!(told, 1, "the person was told again");
    s.done().await;
}
