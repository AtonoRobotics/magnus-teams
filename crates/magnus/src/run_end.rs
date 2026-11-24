//! What a run held only while it ran, let go when it ends: its browsers, its
//! sandbox (with every file it made or imported there) and the bytes of the
//! attachments it fetched. A run's end ends every sub-agent below it, and a
//! sub-agent takes no step once a run above it has ended
//! (decisions/2026-11-23-ending-a-run-ends-its-sub-agents.md).
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{State, Status};
use anyhow::Result;

/// Every sub-agent below `run` that has not ended.
fn going_below(s: &State, run: &str) -> Vec<String> {
    s.sub_agents.below(run).into_iter().map(|(_, a)| a.child).filter(|c| s.runs.get(c).is_some_and(|r| !r.status.terminal())).collect()
}

/// The run above `run` that has ended, if one has: its parent, or one above
/// that. A parent that is no run (a mission lane's requester) never ends.
pub(crate) fn ended_above(s: &State, run: &str) -> Option<String> {
    s.sub_agents.above(run).into_iter().find(|p| s.runs.get(p).is_some_and(|r| r.status.terminal()))
}

/// A sub-agent ended with the run above it: automatic, so no source.
fn cancelled(run: &str) -> Ev {
    Ev::Cancelled { run: run.into(), source: None, turn: None }
}

impl Kernel {
    /// End a run and every sub-agent below it in one write: `evs` holds the
    /// run's own end (its `Cancelled`, and whatever is said with it), and
    /// each sub-agent still going is cancelled with it, so no reader sees
    /// the run ended and a child of it still going. Each run this write
    /// ended then lets go of what it held. Every cancel takes this path.
    pub(crate) async fn cancel_tree(&self, run: &str, evs: Vec<(&str, Ev)>) -> Result<()> {
        self.cancel_trees(&[run.to_string()], evs).await
    }

    /// `cancel_tree` for several runs in one write: `evs` holds each run's
    /// own end, and whatever else must be written with them.
    pub(crate) async fn cancel_trees(&self, runs: &[String], mut evs: Vec<(&str, Ev)>) -> Result<()> {
        let below: Vec<String> = self.read(|s| {
            let mut below: Vec<String> = runs.iter().flat_map(|r| going_below(s, r)).filter(|c| !runs.contains(c)).collect();
            below.sort();
            below.dedup();
            below
        });
        for c in &below {
            evs.push(("kernel", cancelled(c)));
        }
        self.emit_all(evs).await?;
        for r in runs.iter().cloned().chain(below) {
            if self.read(|s| s.runs.get(&r).is_some_and(|x| x.status == Status::Cancelled)) {
                self.on_stop(&r, true).await;
            }
        }
        Ok(())
    }

    /// A run ended, however it ended (done, failed or cancelled): every
    /// sub-agent below it still going is cancelled, and each lets go of
    /// what it held.
    pub(crate) async fn end_run(&self, run: &str) {
        // Stopped or failed, its open outside agent tasks are cancelled (`a2a::outbound`).
        self.outside_run_ended(run).await;
        let below = self.read(|s| going_below(s, run));
        if !below.is_empty() {
            if let Err(e) = self.emit_all(below.iter().map(|c| ("kernel", cancelled(c))).collect()).await {
                tracing::warn!("ending the sub-agents below {run}: {e:#}");
            }
        }
        for r in std::iter::once(run).chain(below.iter().map(String::as_str)) {
            if r != run {
                self.leases.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, holder| holder != r);
            }
            self.release_run(r).await;
            crate::attachment_hold::release(r);
        }
    }

    /// The backstop, before each model call and each action: a sub-agent
    /// with a run above it that has ended asks nothing and does nothing
    /// more. It ends cancelled. True when it was stopped here.
    pub(crate) async fn stopped_from_above(&self, run: &str) -> Result<bool> {
        let going = self.read(|s| s.runs.get(run).is_some_and(|r| !r.status.terminal()));
        if !going || self.read(|s| ended_above(s, run)).is_none() {
            return Ok(false);
        }
        self.emit("kernel", cancelled(run)).await?;
        self.on_stop(run, true).await;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "run_end_tests.rs"]
mod tests;
