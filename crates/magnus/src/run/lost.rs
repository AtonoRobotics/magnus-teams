//! Detect being lost (a removable module): no-progress signals read from the
//! log, over the replies since the last reset. The patterns and their default
//! counts are OpenHands' stuck detector (software-agent-sdk,
//! `StuckDetectionThresholds` in `openhands/sdk/conversation/types.py`,
//! commit 095883242a0b): the same action and result 4 times; the same action
//! failing more than 3 times in a row; 3 replies in a row with no action; two
//! actions alternating over 6 calls with the same results.

use crate::state::Run;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Clone, Debug, serde::Serialize, PartialEq)]
pub struct Thresholds {
    #[serde(default = "action_observation")]
    pub action_observation: usize,
    #[serde(default = "action_error")]
    pub action_error: usize,
    #[serde(default = "monologue")]
    pub monologue: usize,
    #[serde(default = "alternating")]
    pub alternating_pattern: usize,
}

fn action_observation() -> usize {
    4
}
fn action_error() -> usize {
    3
}
fn monologue() -> usize {
    3
}
fn alternating() -> usize {
    6
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds { action_observation: action_observation(), action_error: action_error(), monologue: monologue(), alternating_pattern: alternating() }
    }
}

/// One finished call since the last reset: what was asked and what came back.
#[derive(PartialEq)]
struct Call {
    tool: String,
    input: Value,
    result: String,
    error: bool,
}

/// The no-progress signal in the run's latest steps, if any: which pattern,
/// in words the model can act on, and the reply where the loop began (the
/// checkpoint to go back to).
pub fn signal(run: &Run, t: &Thresholds) -> Option<(String, u32)> {
    let from = run.lost_since as usize;
    let replies = run.replies.get(from..).unwrap_or_default();
    // Replies in a row that asked for nothing.
    let quiet = replies.iter().rev().take_while(|r| r.calls().is_empty()).count();
    if t.monologue > 0 && quiet >= t.monologue {
        let start = (from + replies.len() - quiet) as u32;
        return Some((format!("{quiet} replies in a row called no tool"), start));
    }
    // Finished calls, newest last, with the reply each came from.
    let mut calls: Vec<(usize, Call)> = vec![];
    for (i, r) in replies.iter().enumerate() {
        for (id, name, input) in r.calls() {
            if crate::tools::RECORDS.contains(&name.as_str()) {
                continue;
            }
            let Some(res) = run.results.get(&id) else { continue };
            calls.push((from + i, Call { tool: name, input, result: res.text.clone(), error: res.is_error }));
        }
    }
    let n = calls.len();
    let same = |a: &Call, b: &Call| a.tool == b.tool && a.input == b.input;
    if t.action_observation > 0 && n >= t.action_observation {
        let last = &calls[n - t.action_observation..];
        if last.iter().all(|(_, c)| *c == last[0].1) {
            return Some((format!("the same call ({}) returned the same result {} times in a row", last[0].1.tool, t.action_observation), last[0].0 as u32));
        }
    }
    let streak = calls.iter().rev().take_while(|(_, c)| c.error && same(c, &calls[n - 1].1)).count();
    if t.action_error > 0 && streak > t.action_error {
        let (start, c) = &calls[n - streak];
        return Some((format!("the same call ({}) failed {streak} times in a row with: {}", c.tool, first_line(&c.result)), *start as u32));
    }
    if t.alternating_pattern >= 4 && n >= t.alternating_pattern {
        let last = &calls[n - t.alternating_pattern..];
        let (a, b) = (&last[0].1, &last[1].1);
        if !same(a, b) && last.iter().enumerate().all(|(i, (_, c))| if i % 2 == 0 { c == a } else { c == b }) {
            return Some((format!("two calls ({} and {}) alternated {} times with the same results", a.tool, b.tool, t.alternating_pattern), last[0].0 as u32));
        }
    }
    None
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or_default().chars().take(200).collect()
}

impl crate::kernel::Kernel {
    /// Before the next model call, act on a lost run. With lost-reset on, the
    /// first time: cut the loop out of the context and leave a note. With
    /// escalation on, the next time (or the first, with lost-reset off):
    /// move the run to the stronger model. Lost again with nothing left to
    /// try: suspend for a person. With both modules off this does nothing,
    /// and the run goes on until its budget stops it (their ablation).
    pub(super) async fn when_lost(&self, run: &Run) -> anyhow::Result<Option<super::agent_loop::Flow>> {
        use super::agent_loop::Flow;
        let c = &run.config;
        let (reset, escalate) = (c.module("lost_reset"), c.module("escalation"));
        if !reset && !escalate {
            return Ok(None);
        }
        let Some((signal, from)) = signal(run, &c.lost) else { return Ok(None) };
        let to = run.replies.len() as u32;
        let target = c.escalate_to.as_deref().and_then(|t| t.split_once('/')).filter(|t| *t != run.model());
        let first = run.cuts.is_empty() && run.escalated.is_none();
        if reset && first {
            self.reset(run, from, to, &signal).await?;
            return Ok(Some(Flow::Continue));
        }
        if let Some((adapter, model)) = target.filter(|_| escalate && run.escalated.is_none()) {
            self.emit("kernel", crate::events::Ev::Escalated { run: run.id.clone(), adapter: adapter.into(), model: model.into(), signal: signal.clone() }).await?;
            if reset {
                self.reset(run, from, to, &signal).await?;
            }
            return Ok(Some(Flow::Continue));
        }
        self.suspend_with_notice(
            &run.id,
            "lost",
            format!("Stopped: the agent is going in circles ({signal}) and has nothing left to try. Tell it how to go on, or cancel."),
            serde_json::json!({ "signal": signal, "since_reply": from, "resets": run.cuts.len(), "escalated": run.escalated.is_some(), "model": format!("{}/{}", run.model().0, run.model().1) }),
        )
        .await?;
        Ok(Some(Flow::Stop))
    }

    async fn reset(&self, run: &Run, from: u32, to: u32, signal: &str) -> anyhow::Result<()> {
        // What the cut replies did still happened: say so, by tool.
        let mut done: std::collections::BTreeMap<String, usize> = Default::default();
        for r in run.replies.get(from as usize..to as usize).unwrap_or_default() {
            for (_, name, _) in r.calls() {
                if !crate::tools::RECORDS.contains(&name.as_str()) {
                    *done.entry(name).or_default() += 1;
                }
            }
        }
        let did = if done.is_empty() {
            "They called no tool.".to_string()
        } else {
            format!("Their calls still happened: {}.", done.iter().map(|(n, k)| format!("{n} ×{k}")).collect::<Vec<_>>().join(", "))
        };
        let note = format!(
            "You were going in circles: {signal}. Your {} latest replies were taken out of this conversation. {did} Do not repeat that approach: try a different one, or call task_fail if the task cannot be done.",
            to - from
        );
        self.emit("kernel", crate::events::Ev::ContextReset { run: run.id.clone(), from, to, signal: signal.into(), note }).await?;
        Ok(())
    }
}
