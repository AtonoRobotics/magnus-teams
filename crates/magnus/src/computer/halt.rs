//! A kill switch reaches the agent's computer (spec rev 141, Admin: Kill
//! switch: "Each stops new work and running work at once, including the
//! agent's computer actions"). Pulled, every running computer its work is on
//! is paused as the monitor pauses one, every grant revoked, and marked as
//! held by that switch, so whatever runs inside stops where it is. Released,
//! the computers that switch paused resume, and what ran in them goes on.
//!
//! The server's switch reaches every computer; a person's, their own
//! computer and the computers of the work it stopped; an agent's, the
//! computers of its work (its person's own for a person's agent). A
//! connector's switch leaves the computers: a connector runs in its own
//! microVM, never in the agent's computer, so no computer's work is a
//! connector's (`decisions/2026-11-11-kill-switch-reaches-the-computer.md`).

use super::{client, Change, KillMode, Owner, Role, Status};
use crate::events::Ev;
use crate::halts::{Scope, SERVER_REASON};
use crate::kernel::Kernel;
use anyhow::Result;
use serde_json::json;

impl Kernel {
    /// The owners whose computers a switch below the server's reaches, given
    /// the runs it stopped. `None` for a connector's: it reaches none.
    pub(crate) fn computers_reached(&self, scope: &Scope, stopped: &[String]) -> Option<Vec<Owner>> {
        let mut owners: Vec<Owner> = match scope {
            Scope::Connector(_) => return None,
            Scope::Person(p) => vec![Owner::Person(p.clone())],
            Scope::Agent(a) => a.strip_prefix("person:").map(|p| Owner::Person(p.into())).into_iter().collect(),
        };
        let runs: Vec<crate::state::Run> = self.read(|s| stopped.iter().filter_map(|r| s.runs.get(r).cloned()).collect());
        owners.extend(runs.iter().map(|r| self.owner_of_run(r)));
        Some(owners)
    }

    /// Pauses every running computer of `owners` (all, for `None`), as held
    /// by the switch whose reason is `reason`.
    pub(crate) async fn hold_computers(&self, actor: &str, reason: &str, owners: Option<&[Owner]>) {
        if self.computer_socket().is_err() {
            return;
        }
        let ids: Vec<String> = self.read(|s| {
            s.computers.all().filter(|c| c.role == Role::Computer && c.status == Status::Running && owners.is_none_or(|o| o.contains(&c.owner))).map(|c| c.id.clone()).collect()
        });
        for id in ids {
            let held = Change::HeldBySwitch { id: id.clone(), reason: reason.into() };
            if let Err(e) = self.pause_computer_as(actor, &id, Some(held)).await {
                tracing::warn!("the kill switch could not pause computer {id}: {e:#}");
            }
        }
    }

    /// Resumes the computers the switch whose reason is `reason` paused. One
    /// whose paused VM is gone (its host restarted) is rebuilt on its next
    /// use instead, its files kept.
    pub(crate) async fn release_computers(&self, actor: &str, reason: &str) {
        let Ok(socket) = self.computer_socket() else { return };
        let held: Vec<String> = self.read(|s| s.computers.all().filter(|c| c.status == Status::Paused && c.held_by.as_deref() == Some(reason)).map(|c| c.id.clone()).collect());
        for id in held {
            if let Err(e) = self.resume_held(&socket, actor, &id).await {
                tracing::warn!("computer {id} was not resumed after the kill switch: {e:#}");
            }
        }
    }

    async fn resume_held(&self, socket: &std::path::Path, actor: &str, id: &str) -> Result<()> {
        match client::ask(socket, json!({ "op": "resume", "id": id })).await {
            Ok(_) => {
                self.emit(actor, Ev::Computer(Change::Started { id: id.into() })).await?;
            }
            Err(e) => {
                tracing::warn!("computer {id} could not resume ({e:#}); it is rebuilt on its next use");
                self.emit(actor, Ev::Computer(Change::Killed { id: id.into(), mode: KillMode::Destroy, delete_volume: false })).await?;
                client::ask(socket, json!({ "op": "destroy", "id": id, "delete_volume": false })).await?;
            }
        }
        Ok(())
    }

    /// Whether the switch that paused a computer is still on.
    pub(crate) fn switch_still_on(&self, reason: &str) -> bool {
        self.read(|s| if reason == SERVER_REASON { s.kill_switch } else { s.halts.on.keys().any(|scope| scope.reason() == reason) })
    }
}
