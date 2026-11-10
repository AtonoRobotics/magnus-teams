//! An append to the log and its apply to the state, done whole.
//!
//! The state must never run behind the log: when the log holds an event
//! this process did not apply, the server starts again in place from the
//! log (`log.rs`). A caller dropped between the append and the apply (a
//! client that went away mid-request, a timeout around work that emits)
//! left exactly that, and every such drop restarted the server, failing
//! every open request for seconds: the desktop app's Apps page said the
//! apps were unavailable and stopped answering (magnus-teams, 2026-11-09).
//! So the append and the apply run on a task of their own that finishes
//! whatever happens to its caller, holding the write lock until it has.

use crate::events::Ev;
use crate::log::{Event, Log};
use crate::state::State;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// The log, the state it folds into, and the lock that keeps appends in
/// the order they are applied.
pub struct Writer {
    pub log: Arc<Log>,
    pub state: Arc<Mutex<State>>,
    lock: Arc<tokio::sync::Mutex<()>>,
    /// Debug builds only: the state directory, for `crash-after`.
    crash_dir: Option<PathBuf>,
}

/// What to append: each event with its actor and the person its content
/// belongs to.
pub struct Batch {
    pub evs: Vec<(String, Ev)>,
    pub owners: Vec<Option<String>>,
}

impl Writer {
    pub fn new(log: Log, state: State, crash_dir: Option<PathBuf>) -> Writer {
        Writer { log: Arc::new(log), state: Arc::new(Mutex::new(state)), lock: Arc::new(tokio::sync::Mutex::new(())), crash_dir }
    }

    /// Append and apply what `ready` makes, under the write lock. `ready`
    /// runs under the lock, so it may number events against the state.
    /// Once the lock is held the write goes on to the end, even if this
    /// call is dropped.
    pub async fn write(&self, ready: impl FnOnce(&State) -> Batch) -> Result<Vec<Event>> {
        let guard = self.lock.clone().lock_owned().await;
        let batch = ready(&self.state.lock().unwrap_or_else(PoisonError::into_inner));
        let (log, state, crash_dir) = (self.log.clone(), self.state.clone(), self.crash_dir.clone());
        let whole = tokio::spawn(async move {
            let es = match batch.evs.as_slice() {
                [(actor, ev)] => vec![log.append(actor, ev, batch.owners.first().cloned().flatten().as_deref()).await?],
                evs => {
                    let evs: Vec<(&str, Ev)> = evs.iter().map(|(a, e)| (a.as_str(), e.clone())).collect();
                    log.append_all(&evs, &batch.owners).await?
                }
            };
            crash_after(crash_dir, &es);
            let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
            for e in &es {
                s.apply(e);
            }
            drop(s);
            drop(guard);
            Ok(es)
        });
        whole.await.context("the write to the event log stopped")?
    }
}
/// Debug builds only, for the crash tests: the process ends at once, as a
/// crash would, right after a write that put an event of the kind named in
/// `<state_dir>/crash-after` in the log, and before anything else happens.
/// The file goes first, so the next start runs clean. Release builds, the
/// only ones shipped, have none of this.
#[cfg(debug_assertions)]
fn crash_after(dir: Option<PathBuf>, es: &[Event]) {
    let Some(path) = dir.map(|d| d.join("crash-after")) else { return };
    let Ok(kind) = std::fs::read_to_string(&path) else { return };
    if es.iter().any(|e| serde_json::to_value(&e.ev).is_ok_and(|v| v["kind"] == kind.trim())) {
        let _ = std::fs::remove_file(&path);
        eprintln!("crash-after: ending the process after a {} was written", kind.trim());
        // SAFETY: _exit ends the process without running anything more,
        // which is the crash this stands for.
        unsafe { libc::_exit(70) };
    }
}

#[cfg(not(debug_assertions))]
fn crash_after(_dir: Option<PathBuf>, _es: &[Event]) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::Context as TaskContext;

    /// A fresh database with only the event log in it.
    async fn log_db() -> (tokio_postgres::Client, String, String) {
        let server = std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into());
        let name = format!("w_{}", uuid::Uuid::new_v4().simple());
        let admin = crate::log::connect(&server).await.unwrap();
        admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
        let mut u = url::Url::parse(&server).unwrap();
        u.set_path(&format!("/{name}"));
        let url = u.to_string();
        let c = crate::log::connect(&url).await.unwrap();
        c.batch_execute("CREATE TABLE public.events (seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, at timestamptz NOT NULL DEFAULT now(), kind text NOT NULL, actor text NOT NULL, run_id text, body jsonb NOT NULL)").await.unwrap();
        (admin, name, url)
    }

    fn boot() -> Batch {
        Batch { evs: vec![("kernel".into(), Ev::KernelBoot { version: "t".into(), pid: 1, instructions: "t".into() })], owners: vec![None] }
    }

    /// A write whose caller goes away once the append is on its way, as a
    /// request does when its client hangs up, is still applied: the state
    /// keeps up with the log, and the next write follows on without the
    /// gap that would restart the server.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_write_whose_caller_is_dropped_is_still_applied() {
        let (admin, name, url) = log_db().await;
        let w = Writer::new(Log::open(&url).await.unwrap(), State::default(), None);
        let c = crate::log::connect(&url).await.unwrap();
        let in_db = || async { c.query_one("SELECT count(*) FROM public.events", &[]).await.unwrap().get::<_, i64>(0) };
        let mut write = Box::pin(w.write(|_| boot()));
        // Drive the write by hand until the database holds the event, then
        // drop it there: the append is committed and only the apply is left,
        // as when a client hangs up between the two.
        let waker = std::task::Waker::noop();
        // The database is asked before each poll, so the poll that would
        // collect the answer and apply it never comes once it is there.
        let mut done = write.as_mut().poll(&mut TaskContext::from_waker(waker)).is_ready();
        for _ in 0..500 {
            if done {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if in_db().await == 1 {
                break;
            }
            done = write.as_mut().poll(&mut TaskContext::from_waker(waker)).is_ready();
        }
        assert!(!done, "the write finished before it could be dropped");
        drop(write);
        let mut in_log = 0i64;
        for _ in 0..200 {
            in_log = in_db().await;
            if in_log == 1 && w.state.lock().unwrap().last_seq == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(in_log, 1, "the dropped write never reached the log");
        assert_eq!(w.state.lock().unwrap().last_seq, 1, "the log holds an event the state never applied: the next write restarts the server");
        let next = w.write(|_| boot()).await.unwrap();
        assert_eq!(next[0].seq, 2);
        drop(in_db);
        drop(c);
        drop(w);
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await.unwrap();
    }
}
