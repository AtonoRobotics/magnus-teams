//! A run's allowance: whether it may spend more, and how much.
//!
//! Every paid step of a run asks here once: the agent loop before each
//! model call, the boundary before each action the model asked for, and
//! the kernel's optional extra calls (reviewer, memory pass, knowledge
//! extraction, spoken script) before theirs. One place holds every rule:
//!
//! - the run's own budget: its model calls, its actions and its dollars;
//! - its grant's budget for the period (#280, #286);
//! - the month's limits, the person's and the server's (2026-11-10);
//! - its team's, the workspace's and the channel's (2026-11-13).
//!
//! The limits hold partway through a run for every spender alike. Only the
//! call cap differs by spender: the agent's steps are counted apart from the
//! kernel's extras, which never use them up but skip themselves once the
//! run's total calls reach the cap (2026-11-09). Actions are bounded by the
//! action cap instead.

use crate::boundary::{clamp_output_for_budget, estimated_input_tokens};
use crate::kernel::Kernel;
use crate::state::{Run, State};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Who asks to spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spender {
    /// The agent loop's next model call: one of the agent's own steps.
    Step,
    /// One of the kernel's optional model calls: the agent's steps are not
    /// its to use, but it makes none once the run's total reaches the cap.
    Extra,
    /// An action the model asked for, bounded by the action cap.
    Action,
}

/// What the run may still spend.
#[derive(Debug, Clone, PartialEq)]
pub struct Left {
    /// The run's own dollars left, which a call's worst case must fit.
    pub usd: f64,
}

/// Why the run may spend no more: the stop's reason, its cause, the notice
/// the person reads, and the details the app reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    /// `budget`, `grant_budget` or `spend_limit`.
    pub reason: &'static str,
    pub why: String,
    pub notice: String,
    pub details: Value,
}

/// What a run has spent of its action and dollar budget, as a budget stop
/// tells the person.
pub fn budget_used(run: &Run) -> String {
    let b = &run.config.budget;
    format!("budget used: {} of {} actions, ${:.2} of ${:.2}", run.actions, b.max_actions, run.spent_usd(), b.max_usd)
}

/// Whether `run` may spend once more for `spender` at `now`, and what it
/// has left; a run that is not there has nothing.
pub fn allowance(s: &State, run_id: &str, spender: Spender, now: DateTime<Utc>) -> Result<Left, Box<Stop>> {
    let Some(run) = s.runs.get(run_id) else {
        return Err(Box::new(Stop { reason: "budget", why: "the run is not there".into(), notice: String::new(), details: Value::Null }));
    };
    let budget = &run.config.budget;
    let calls = match spender {
        Spender::Step => Some(run.loop_calls()),
        Spender::Extra => Some(run.model_calls),
        Spender::Action => None,
    };
    if let Some(n) = calls.filter(|n| *n >= budget.max_model_calls) {
        let why = format!("{n} of {} model calls", budget.max_model_calls);
        let notice = format!("Stopped after {n} model calls, the task's limit.");
        return Err(Box::new(Stop { reason: "budget", why, notice, details: json!({ "model_calls": n, "limit": budget.max_model_calls, "usd": run.spent_usd() }) }));
    }
    // Dollars are spent by model calls, failed ones too: one past the limit
    // stops the run before the next is paid for.
    let actions_spent = spender == Spender::Action && run.actions >= budget.max_actions;
    if actions_spent || run.spent_usd() >= budget.max_usd {
        let why = budget_used(run);
        let notice = format!("Stopped: {why}.");
        return Err(Box::new(Stop { reason: "budget", why, notice, details: json!({ "actions": run.actions, "usd": run.spent_usd() }) }));
    }
    // A task under a grant stops once the grant's budget for its period is
    // spent, by all its tasks together, until its person resumes it.
    if let Some(crate::grants::Spent { grant: g, spent, max, period }) = crate::grants::spent_for_run(s, run_id, now) {
        let why = format!("the grant {g} has spent its ${max:.2} for this {period} (${spent:.2}).");
        let notice = format!("Stopped: {why}");
        return Err(Box::new(Stop { reason: "grant_budget", why, notice, details: json!({ "grant": g, "usd": spent, "limit": max }) }));
    }
    // The month's spend limits, the person's and the server's, whoever
    // extended the run's budget.
    if let Err(stop) = super::enforce::left_now(s, &run.person, now) {
        let why = stop.to_string();
        return Err(Box::new(Stop { reason: "spend_limit", notice: format!("Stopped: {why}"), why, details: super::enforce::details(&stop) }));
    }
    // A team's, its workspace's and its channel's, when they are lowered or
    // overrun; a sub-agent's through the run at its top.
    if let Some((why, details)) = crate::team::spend::stop_partway(s, run_id, now) {
        return Err(Box::new(Stop { reason: "spend_limit", notice: format!("Stopped: {why}"), why, details }));
    }
    Ok(Left { usd: budget.max_usd - run.spent_usd() })
}

impl Kernel {
    /// The run's allowance now, read from the state as it is.
    pub(crate) fn allowance(&self, run: &str, spender: Spender) -> Result<Left, Box<Stop>> {
        self.read(|s| allowance(s, run, spender, Utc::now()))
    }

    /// An extra model call's output allowance: the full allowance when the
    /// run's allowance admits an extra and the call's worst case fits what
    /// is left, None when not — the caller skips the call. Extra calls are
    /// optional (verdicts, memory, extractions, spoken scripts), never the
    /// run's work, so they run whole or not at all: a truncated judgment is
    /// worse than none, and the run is never suspended for one (#319). An
    /// estimate the adapter cannot give falls back to the kernel's bytes; a
    /// quote it cannot give skips: the call would fail anyway.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn aux_max_tokens(
        &self,
        run_id: &str,
        purpose: &str,
        adapter: &str,
        model: &str,
        system: &str,
        messages: Vec<magnus_adapter_proto::Message>,
        tools: Vec<magnus_adapter_proto::ToolSpec>,
        max_tokens: u32,
    ) -> Option<u32> {
        let left = match self.allowance(run_id, Spender::Extra) {
            Ok(l) => l.usd,
            Err(stop) => {
                tracing::info!("{purpose} for {run_id} skipped: {}", stop.why);
                return None;
            }
        };
        let input_tokens = match self.adapters.estimate_prompt(adapter, model, system, messages.clone(), tools.clone()).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("{purpose} prompt estimate for {run_id}: {e:#}; pricing the kernel's bytes");
                estimated_input_tokens(system, &messages, &tools)
            }
        };
        let (input_usd, output_usd) = match self.adapters.quote(adapter, model, input_tokens, max_tokens).await {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("{purpose} quote for {run_id}: {e:#}; skipping");
                return None;
            }
        };
        match clamp_output_for_budget(left, input_usd, output_usd, max_tokens) {
            Some(n) if n == max_tokens => Some(n),
            _ => {
                tracing::info!("{purpose} for {run_id} skipped: its worst case does not fit what is left");
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "allowance_tests.rs"]
mod tests;
