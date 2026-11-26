//! A kernel booted on a fresh database with a fake computerd, for the
//! computer tests.

use super::fake_computerd::Fake;
use super::{Change, Computer, Owner, Role};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::legal::TEST_CONFIG as LEGAL;
use serde_json::Value;
use std::sync::Arc;

fn pg() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

pub(super) struct Server {
    pub k: Arc<Kernel>,
    /// Creates, wakes, destroys and erases at once; its guests answer every
    /// desktop action with a screenshot.
    pub fake: Fake,
    db: String,
    _dir: tempfile::TempDir,
}

/// sam owns the server, ana is an admin and mo a member; the server has
/// computers.
pub(super) async fn server() -> Server {
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
         CREATE TABLE public.content (seq bigint PRIMARY KEY REFERENCES public.events (seq), salt bytea NOT NULL, body jsonb NOT NULL, person text);",
    )
    .await
    .unwrap();
    c.batch_execute(&crate::accounts::migrate_sql("postgres")).await.unwrap();
    let socket = dir.path().join("computerd.sock");
    let fake = Fake::at(&socket);
    let p = dir.path().join("magnus.toml");
    let state = dir.path().join("state");
    std::fs::write(
        &p,
        format!(
            "[appliance]\nstate_dir = {state:?}\nlisten = \"127.0.0.1:0\"\n[database]\nurl = {url:?}\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[computer]\nsocket = {socket:?}\n\
             [[people]]\nid = \"sam\"\nname = \"Sam\"\nroles = [\"owner\"]\n[[people]]\nid = \"ana\"\nname = \"Ana\"\nroles = [\"admin\"]\n[[people]]\nid = \"mo\"\nname = \"Mo\"\n"
        ),
    )
    .unwrap();
    let cfg = crate::config::Config::load(&p).unwrap();
    crate::memory::schema::migrate(&url, &url, &cfg.memory).await.unwrap();
    let k = Kernel::boot(cfg).await.unwrap();
    Server { k, fake, db: name, _dir: dir }
}

impl Server {
    pub async fn done(self) {
        let admin = crate::log::connect(&pg()).await.unwrap();
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.db)).await.unwrap();
    }

    pub fn computers_of(&self, owner: &Owner) -> Vec<Computer> {
        self.k.read(|s| s.computers.all().filter(|c| c.role == Role::Computer && c.owner == *owner).cloned().collect())
    }

    pub fn asked(&self, op: &str) -> Vec<Value> {
        self.fake.asked_op(op)
    }

    /// A computer made the way it was before rev 142: by hand, one of several.
    pub async fn made_by_hand(&self, owner: &Owner) -> String {
        let id = super::service::new_id();
        self.k.emit("person:sam", Ev::Computer(Change::Created { id: id.clone(), owner: owner.clone() })).await.unwrap();
        id
    }
}

pub(super) fn person(id: &str) -> Owner {
    Owner::Person(id.into())
}

/// A workspace's computer where sam (its Team Admin) and mo (a member) may
/// both take control.
pub(super) async fn shared_computer(s: &Server) -> String {
    let ws = "w1";
    s.k.emit("person:sam", Ev::Team(crate::team::Change::Created { workspace: ws.into(), name: "Ops".into(), owner: "sam".into() })).await.unwrap();
    s.k.emit("person:sam", Ev::Team(crate::team::Change::Member { workspace: ws.into(), person: "mo".into(), role: crate::team::Role::Member })).await.unwrap();
    s.k.reconcile_computers().await;
    s.computers_of(&Owner::Workspace(ws.into()))[0].id.clone()
}
