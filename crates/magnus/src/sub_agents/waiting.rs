//! A run asking after its own background sub-agent while that one still
//! works waits for it, in the kernel, instead of asking again and again.
//! Each ask is a model call that sends the run's whole context: on
//! 2026-11-25 a chat turn asked after its three sub-agents 74 times in five
//! minutes, about three million input tokens spent on "still working".

use super::CHILD_POLL;
use crate::kernel::Kernel;
use crate::state::{State, Status};
use std::time::{Duration, Instant};

/// The longest one `get_task` on a run's own working sub-agent waits. Past
/// it the row says the sub-agent is still working, as before.
pub(crate) const OWN_CHILD_WAIT: Duration = Duration::from_secs(10 * 60);

/// The run of `task` when it is a sub-agent `run` started itself.
fn own_child(s: &State, run: &str, task: &str) -> Option<String> {
    let child = s.tasks.get(task)?.run_id.clone();
    s.sub_agents.get(&child).filter(|sa| sa.parent == run).map(|_| child)
}

/// Still working: neither ended nor waiting on a person.
fn working(s: &State, run: &str) -> bool {
    s.runs.get(run).is_some_and(|r| !r.status.terminal() && r.status != Status::Suspended)
}

impl Kernel {
    /// Waits, up to `limit`, while `task` is a sub-agent of `run` that is
    /// still working and `run` itself has not ended. Any other task, and a
    /// sub-agent that has finished or needs a person, returns at once.
    pub(crate) async fn await_own_child(&self, run: &str, task: &str, limit: Duration) {
        let Some(child) = self.read(|s| own_child(s, run, task)) else { return };
        let started = Instant::now();
        while started.elapsed() < limit && self.read(|s| working(s, &child) && s.runs.get(run).is_some_and(|r| !r.status.terminal())) {
            tokio::time::sleep(CHILD_POLL).await;
        }
    }
}

#[cfg(test)]
#[path = "waiting_tests.rs"]
mod tests;
