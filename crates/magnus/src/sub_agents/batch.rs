//! A batch of sub-agents is counted before it starts
//! (decisions/2026-11-23-a-batch-of-sub-agents-is-counted-before-it-starts.md).
//!
//! Consecutive `start_sub_agent` and `research_web` calls in one reply run
//! at once (`run::agent_loop`). Each checks the run's cap on sub-agents and
//! its action budget, then waits on other work before its child or its
//! action is recorded, so calls checked one by one would all pass before
//! any of them counted. The batch is admitted here instead, in one step:
//! as many calls go ahead, first to last, as the run may still start
//! sub-agents and take actions (its tree's and every run's above it), and
//! those actions are held (`spend::holds`) until the batch is done, so no
//! sub-agent takes them first. The rest are refused before any starts.

use crate::kernel::Kernel;
use crate::spend::allowance::scope_of_run;
use crate::spend::holds::{Held, Hold};

/// A batch admitted: how many of its calls go ahead, and why the rest do
/// not. It holds their actions until it is dropped.
pub(crate) struct Batch<'k> {
    /// How many of the calls, first to last, go ahead.
    pub go: usize,
    /// The stage and the reason the rest are refused with. None when no
    /// action is left at all: the boundary stops the run then, as for any
    /// action past the budget.
    pub refused: Option<(&'static str, String)>,
    _held: Held<'k>,
}

/// How many of `asked` calls go ahead, given the sub-agents the run may
/// still start (`room`) and the actions it has left, and why the rest are
/// refused.
fn fit(asked: usize, room: Result<usize, String>, actions: usize) -> (usize, Option<(&'static str, String)>) {
    let children = room.as_ref().map_or(0, |n| *n);
    let go = asked.min(children).min(actions);
    let refused = match room {
        _ if go == asked || actions == 0 => None,
        Err(why) => Some(("sub_agents", why.trim_end_matches('.').to_string())),
        Ok(n) if n <= actions => Some((
            "sub_agents",
            format!(
                "one run may start {} sub-agents and this one may start {n} more, so only the first {n} of these {asked} calls were started and this one was not. Finish the work with them or yourself",
                super::MAX_CHILDREN_PER_RUN
            ),
        )),
        Ok(_) => Some(("budget", format!("{actions} actions are left of this task's budget, so only the first {actions} of these {asked} calls were started and this one was not"))),
    };
    (go, refused)
}

impl Kernel {
    /// Admit a batch of `asked` delegation calls of `run`'s, made at once.
    pub(crate) fn admit_delegations(&self, run: &str, asked: usize) -> Batch<'_> {
        let ((go, refused), held) = self.spend_holds.reserve(|held| {
            self.read(|s| {
                let scope = scope_of_run(s, run);
                let actions = crate::spend::tree::actions_left(s, &scope.runs, held);
                let (go, refused) = fit(asked, s.sub_agents.room(run), usize::try_from(actions).unwrap_or(usize::MAX));
                let hold = Hold { actions: u32::try_from(go).unwrap_or(u32::MAX), actions_at: s.runs.get(run).map_or(0, |r| r.actions), ..scope };
                ((go, refused), hold)
            })
        });
        Batch { go, refused, _held: held }
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
