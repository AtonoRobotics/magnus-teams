//! One computer, always there: against a kernel booted on a fresh database
//! and a computerd that answers at once and remembers what it was asked.

use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

type Asked = Arc<Mutex<Vec<Value>>>;

/// Creates, wakes and destroys at once; its guests answer every desktop
/// action with a screenshot.
fn fake_computerd(socket: &Path) -> Asked {
    let asked: Asked = Arc::default();
    let listener = UnixListener::bind(socket).unwrap();
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let req: Value = serde_json::from_str(&line).unwrap();
                    log.lock().unwrap().push(req.clone());
                    let reply = match req["op"].as_str().unwrap() {
                        "wake" => json!({ "ok": true, "resumed": true, "overlay_fresh": false }),
                        "create" | "destroy" | "connect" | "ping" | "hibernate" => json!({ "ok": true }),
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

/// Destroy became reset: erasing the files puts a new computer in the old
/// one's place, and only its own person may erase them.
#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_computer_leaves_a_new_one_never_none() {
    let s = server().await;
    s.k.reconcile_computers().await;
    let old = s.computers_of(&person("mo"))[0].id.clone();
    let sam = s.k.person("sam").unwrap();
    assert!(s.k.kill_computer(&sam, &old, KillMode::Destroy, true).await.is_err(), "an admin erased a person's files");
    let mo = s.k.person("mo").unwrap();
    let reset = s.k.kill_computer(&mo, &old, KillMode::Destroy, true).await.unwrap();
    let now = s.computers_of(&person("mo"));
    assert_eq!(now.len(), 1, "the person was left with {} computers", now.len());
    assert_eq!(reset["id"], json!(now[0].id));
    assert_ne!(now[0].id, old);
    assert!(s.asked("destroy").iter().any(|r| r["id"] == json!(old) && r["delete_volume"] == true), "the old computer was not deleted");
    let again = s.k.reset_computer(&mo, &now[0].id, false).await.unwrap();
    assert_eq!(s.computers_of(&person("mo")).len(), 1);
    assert_eq!(again["replaces"], json!(now[0].id));
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
