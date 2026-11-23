//! A kill switch stops everything the work it stopped was using, and a
//! run's end releases it: one place for both (`stop_work`, `release_run`;
//! `decisions/2026-11-23-computers-agree-with-computerd.md`).
//!
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

use super::client::Request;
use super::{client, Change, KillMode, Owner, Role, Status};
use crate::events::Ev;
use crate::halts::{Scope, SERVER_REASON};
use crate::kernel::Kernel;
use anyhow::Result;

/// Whose work a kill switch stops.
pub(crate) enum Reach<'a> {
    /// The server's switch: everyone's.
    Everyone,
    /// These owners' computers and browsers, beside the runs it stopped.
    Owners(&'a [Owner]),
    /// A connector's: the runs it stopped, and no computer.
    Nobody,
}

impl Kernel {
    /// Everything the runs a kill switch stopped, and the owners it
    /// reaches, were using, stopped where it is: the host browser's pages,
    /// the runs' throwaway browsers (deleted, so a run continued later reads
    /// on a blank one), the owners' signed-in browsers (stopped, their
    /// profiles sealed), the runs' sandboxes, and the owners' computers
    /// (paused, held by the switch).
    pub(crate) async fn stop_work(&self, actor: &str, reason: &str, runs: &[String], reach: Reach<'_>) {
        match reach {
            Reach::Everyone => self.browser.close_all().await,
            _ => {
                for run in runs {
                    self.browser.close(run).await;
                }
            }
        }
        for run in runs {
            self.sandboxes.stop(run).await;
        }
        let Ok(socket) = self.computer_socket() else { return };
        let reached = |o: &Owner| match reach {
            Reach::Everyone => true,
            Reach::Owners(owners) => owners.contains(o),
            Reach::Nobody => false,
        };
        let (throwaways, signed_in): (Vec<String>, Vec<super::Computer>) = self.read(|s| {
            let browsers: Vec<&super::Computer> = s.computers.all().filter(|c| c.role != Role::Computer).collect();
            let throwaways = browsers.iter().filter(|c| matches!(&c.role, Role::Throwaway { run } if runs.contains(run) || reached(&c.owner))).map(|c| c.id.clone()).collect();
            let signed_in = browsers.iter().filter(|c| matches!(c.role, Role::Identity { .. }) && c.status == Status::Running && reached(&c.owner)).map(|c| (*c).clone()).collect();
            (throwaways, signed_in)
        });
        let gone: Vec<(&str, crate::events::Ev)> =
            throwaways.iter().map(|id| (actor, Ev::Computer(Change::Killed { id: id.clone(), mode: KillMode::Destroy, delete_volume: true }))).collect();
        if !gone.is_empty() {
            if let Err(e) = self.delete_computers(gone).await {
                tracing::warn!("the kill switch could not remove throwaway browsers {throwaways:?}: {e:?}");
            }
        }
        for c in signed_in {
            if let Err(e) = self.stop_browser(&socket, &c).await {
                tracing::warn!("the kill switch could not stop browser {}: {e:#}", c.id);
            }
        }
        match reach {
            Reach::Everyone => self.hold_computers(actor, reason, None).await,
            Reach::Owners(owners) => self.hold_computers(actor, reason, Some(owners)).await,
            Reach::Nobody => {}
        }
    }

    /// A run's end takes what it used with it: its pages in the host
    /// browser, its throwaway browser and its sandbox.
    pub(crate) async fn release_run(&self, run: &str) {
        self.browser.close(run).await;
        let throwaway = self.read(|s| s.computers.all().find(|c| matches!(&c.role, Role::Throwaway { run: r } if r == run)).map(|c| c.id.clone()));
        if let Some(id) = throwaway.filter(|_| self.computer_socket().is_ok()) {
            let gone = vec![("kernel", Ev::Computer(Change::Killed { id, mode: KillMode::Destroy, delete_volume: true }))];
            if let Err(e) = self.delete_computers(gone).await {
                tracing::warn!("the throwaway browser of {run} was not removed: {e:?}");
            }
        }
        self.sandboxes.destroy(run).await;
    }

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
        match client::ask(socket, Request::Resume { id: id.to_string() }).await {
            Ok(_) => {
                self.emit(actor, Ev::Computer(Change::Started { id: id.into() })).await?;
            }
            Err(e) => {
                tracing::warn!("computer {id} could not resume ({e:#}); it is rebuilt on its next use");
                self.emit(actor, Ev::Computer(Change::Killed { id: id.into(), mode: KillMode::Destroy, delete_volume: false })).await?;
                client::ask(socket, Request::Destroy { id: id.to_string(), delete_volume: false }).await?;
            }
        }
        Ok(())
    }

    /// Whether the switch that paused a computer is still on.
    pub(crate) fn switch_still_on(&self, reason: &str) -> bool {
        self.read(|s| if reason == SERVER_REASON { s.vault.kill_switch() } else { s.halts.on.keys().any(|scope| scope.reason() == reason) })
    }
}
