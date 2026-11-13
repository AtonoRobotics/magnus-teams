//! Pulling and releasing a switch below the server's, and pausing an agent,
//! as the kernel's own operations. Who may is the caller's to decide.
use super::{Change, Scope};
use crate::events::Ev;
use crate::kernel::{Kernel, KillSwitchOn};
use crate::state::Status;
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;

/// New work refused because a switch below the server's, or a pause, holds
/// it: what is said to whoever asked.
#[derive(Debug)]
pub struct Halted(pub String);

impl std::fmt::Display for Halted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Halted {}

/// Whether an error is new work refused by a switch or a pause.
pub fn is_refusal(e: &anyhow::Error) -> bool {
    e.is::<KillSwitchOn>() || e.is::<Halted>()
}

/// The answer to new work a switch or a pause refused.
pub fn refused(e: &anyhow::Error) -> Option<axum::response::Response> {
    is_refusal(e).then(|| crate::api::err(StatusCode::CONFLICT, e.to_string()))
}

type Answer = Result<Value, (StatusCode, Value)>;

fn failed(e: anyhow::Error) -> (StatusCode, Value) {
    (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") }))
}

/// One switch at a time: a release racing a pull of the same one could
/// leave a run stopped after the release, untold.
static SWITCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl Kernel {
    /// New work for `person` (by the team agent when `team`), refused when
    /// a switch, a pause or their account turned off holds it: the
    /// server's first.
    pub fn refuse_new(&self, person: &str, team: bool) -> Option<anyhow::Error> {
        self.read(|s| super::may_proceed(s, &super::Subject::new_work(person, team), super::Phase::Start)).map(|h| h.error())
    }

    /// What holds `who`'s work at `phase` now, if anything does.
    pub fn hold(&self, who: &super::Subject, phase: super::Phase) -> Option<super::Hold> {
        self.read(|s| super::may_proceed(s, who, phase))
    }

    /// The switch that holds a run now, if one does: the reason it waits
    /// with, and what is said of it. With `tool`, that tool's connector too.
    pub fn halt_of(&self, run: &str, tool: Option<&str>) -> Option<(String, String)> {
        self.read(|s| s.runs.get(run).and_then(|r| super::halt(s, r, tool)))
    }

    /// Pull or release one switch below the server's, as `actor` (`name`,
    /// as people see them). Pulled, every running run under it stops at
    /// once, with the pages and microVMs it was using; releasing it tells
    /// each person which of their tasks it stopped, in the same write, and
    /// resumes nothing (K53).
    pub async fn scoped_switch(self: &Arc<Self>, actor: &str, name: &str, scope: Scope, engaged: bool) -> Answer {
        let _one = SWITCHING.lock().await;
        let on = self.read(|s| s.halts.on.contains_key(&scope));
        if !engaged {
            if !on {
                return Err((StatusCode::CONFLICT, json!({ "error": format!("{} is not on", scope.said()) })));
            }
            return self.release(actor, scope).await;
        }
        if !on {
            self.emit(actor, Ev::Halt(Change::Engaged { scope: scope.clone(), name: name.into() })).await.map_err(failed)?;
        }
        // Every running run under it, now. One that starts a call after this
        // read meets the switch at the boundary.
        let running: Vec<String> = self.read(|s| s.runs.values().filter(|r| r.status == Status::Running && super::covers(s, &scope, r)).map(|r| r.id.clone()).collect());
        let reason = scope.reason();
        for run in &running {
            self.suspend(run, &reason, None).await.map_err(failed)?;
            self.browser.close(run).await;
        }
        match &scope {
            Scope::Person(p) => {
                self.brokered.stop(None, Some(p)).await;
            }
            Scope::Agent(a) => {
                if let Some(p) = a.strip_prefix("person:") {
                    self.brokered.stop(None, Some(p)).await;
                }
            }
            Scope::Connector(c) => {
                self.brokered.stop(Some(c), None).await;
            }
        }
        // And the computers its work is on, paused where they are.
        if let Some(owners) = self.computers_reached(&scope, &running) {
            self.hold_computers(actor, &reason, Some(&owners)).await;
        }
        Ok(json!({ "engaged": true, "level": scope.level(), "id": scope.id(), "stopped": running }))
    }

    async fn release(self: &Arc<Self>, actor: &str, scope: Scope) -> Answer {
        let reason = scope.reason();
        let stopped: Vec<crate::state::Run> =
            self.read(|s| s.runs.values().filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some(reason.as_str())).cloned().collect());
        let why = json!({ "reason": reason, "evidence": { "why": format!("{} stopped it, and has been released; Continue goes on from where it stopped", scope.said()) } });
        let mut evs = vec![(actor, Ev::Halt(Change::Released { scope: scope.clone() }))];
        let mut told = 0;
        for r in &stopped {
            // Background work is not the person's to continue: it ends.
            if self.is_background(&r.id) {
                evs.push(("kernel", Ev::Failed { run: r.id.clone(), reason: "background work stopped by a kill switch".into(), by: "kernel".into() }));
                continue;
            }
            told += 1;
            let title = self.title_of(r).await;
            evs.push(("kernel", self.notice_of(r, format!("Stopped by {}: {title}", scope.said()), why.clone())));
        }
        self.emit_all(evs).await.map_err(failed)?;
        self.release_computers(actor, &reason).await;
        for r in stopped.iter().filter(|r| self.is_background(&r.id)) {
            self.on_stop(&r.id, true).await;
        }
        Ok(json!({ "engaged": false, "level": scope.level(), "id": scope.id(), "told": told }))
    }

    /// Pause an agent (nothing new starts for it; what runs goes on), or
    /// resume it.
    pub async fn pause_agent(&self, actor: &str, name: &str, agent: &str, paused: bool) -> Answer {
        if Scope::parse("agent", agent).is_none() {
            return Err((StatusCode::NOT_FOUND, json!({ "error": "no such agent" })));
        }
        let now = self.read(|s| s.halts.paused.contains_key(agent));
        let ev = match (paused, now) {
            (true, false) => Change::Paused { agent: agent.into(), name: name.into() },
            (false, true) => Change::Unpaused { agent: agent.into() },
            _ => return Ok(json!({ "agent": agent, "paused": paused })),
        };
        self.emit(actor, Ev::Halt(ev)).await.map_err(failed)?;
        Ok(json!({ "agent": agent, "paused": paused }))
    }
}
