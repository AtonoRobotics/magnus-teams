//! Sub-agents: child runs with their own context, a goal and a share of the
//! parent's budget. A child holds at most its parent's authority, keeps its
//! person, and its reading taints the parent.
use crate::boundary::Action;
use crate::config::Budget;
use crate::kernel::{Kernel, NewRun};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_DEPTH: usize = 3;
const MAX_CHILD_SHARE: f64 = 0.5;
const DEFAULT_CHILD_SHARE: f64 = 0.25;
const CHILD_WAIT: Duration = Duration::from_secs(30 * 60);
const CHILD_POLL: Duration = Duration::from_millis(250);

/// Recorded with the child, so its limits outlive the parent's tool call.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SubAgent {
    pub child: String,
    pub parent: String,
    /// The tools it may use; empty means the parent's own.
    pub tools: Vec<String>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    children: BTreeMap<String, SubAgent>,
}

impl Ledger {
    pub fn apply(&mut self, s: &SubAgent) {
        self.children.insert(s.child.clone(), s.clone());
    }

    pub fn get(&self, child: &str) -> Option<&SubAgent> {
        self.children.get(child)
    }

    fn depth(&self, run: &str) -> usize {
        let mut depth = 0;
        let mut at = run;
        while let Some(s) = self.children.get(at) {
            depth += 1;
            at = &s.parent;
            if depth > MAX_DEPTH {
                break;
            }
        }
        depth
    }

    /// Why a sub-agent may not use this tool, if so: its own limit and every
    /// limit above it.
    pub fn refusal(&self, run: &str, tool: &str) -> Option<String> {
        let mut at = run;
        for _ in 0..=MAX_DEPTH {
            let s = self.children.get(at)?;
            if !s.tools.is_empty() && !s.tools.iter().any(|t| t == tool) {
                return Some(format!("this sub-agent was not given {tool}"));
            }
            at = &s.parent;
        }
        None
    }
}

/// A share of the parent's budget, however it was asked for.
fn child_budget(parent: &Budget, share: Option<f64>) -> Budget {
    let share = share.unwrap_or(DEFAULT_CHILD_SHARE).clamp(0.0, MAX_CHILD_SHARE);
    Budget {
        max_model_calls: ((f64::from(parent.max_model_calls) * share) as u32).max(1),
        max_actions: ((f64::from(parent.max_actions) * share) as u32).max(1),
        max_usd: parent.max_usd * share,
    }
}

impl Kernel {
    pub(crate) async fn start_sub_agent(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let parent = self.read(|s| s.runs.get(&a.run).cloned()).context("no such run")?;
        let goal = a.input["goal"].as_str().context("goal is required")?.to_string();
        if self.read(|s| s.sub_agents.depth(&a.run)) >= MAX_DEPTH {
            return Ok(("Sub-agents cannot be nested this deeply.".to_string(), true, None));
        }
        let parent_grant = self.read(|s| s.tasks.get(&parent.task).and_then(|t| t.grant.clone()));
        let grant = a.input["grant"].as_str().map(String::from);
        if grant.is_some() && grant != parent_grant {
            return Ok(("That grant is not one this run holds, so a sub-agent cannot be given it.".to_string(), true, None));
        }
        let mut tools: Vec<String> = a.input["tools"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
        // A child never gets to start more than its own limit lets it.
        let ceiling = self.read(|s| s.sub_agents.get(&a.run).map(|p| p.tools.clone())).unwrap_or_default();
        if !ceiling.is_empty() {
            if let Some(extra) = tools.iter().find(|t| !ceiling.contains(t)) {
                return Ok((format!("{extra} is not a tool this sub-agent holds."), true, None));
            }
            if tools.is_empty() {
                tools = ceiling;
            }
        }
        // What the parent has read comes along in the goal it writes.
        let taint = crate::tools::Label { untrusted: parent.untrusted, private: parent.private };
        let o = NewRun {
            label: (taint.untrusted || taint.private).then_some(taint),
            budget: Some(child_budget(&parent.config.budget, a.input["budget_share"].as_f64())),
            grant,
            model: Some((parent.config.adapter.clone(), parent.config.model.clone())),
            ..Default::default()
        };
        let (task, child) = self.create_run(&format!("run:{}", a.run), &parent.person, goal, vec![], o).await?;
        self.emit("kernel", crate::events::Ev::SubAgent(SubAgent { child: child.clone(), parent: a.run.clone(), tools })).await?;
        if let Some(t) = self.team_of(&parent.task) {
            let scope = crate::team::Change::Run {
                workspace: t.workspace,
                channel: t.channel,
                thread: t.thread,
                task: task.clone(),
                requester: t.requester,
                history: t.history,
                sub: true,
                outside: t.outside,
            };
            self.emit("kernel", crate::events::Ev::Team(scope)).await?;
        }
        self.wake(&child);
        self.wait_for_child(&task, &child).await
    }

    async fn wait_for_child(&self, task: &str, child: &str) -> Result<(String, bool, Option<Value>)> {
        let started = Instant::now();
        loop {
            let Some(r) = self.read(|s| s.runs.get(child).cloned()) else { return Err(anyhow!("the sub-agent's run is gone")) };
            if r.status.terminal() || r.status == crate::state::Status::Suspended {
                let taint = json!({ "untrusted": r.untrusted, "private": r.private, "child_run": child, "task": task });
                let said = r.summary.clone().unwrap_or_else(|| "(it left no summary)".to_string());
                let text = if r.status.terminal() { said } else { format!("The sub-agent is waiting on a person, in run {child}. It carries on once they answer.") };
                return Ok((text, r.status == crate::state::Status::Failed, Some(taint)));
            }
            if started.elapsed() > CHILD_WAIT {
                return Ok((format!("The sub-agent in run {child} is still working."), true, Some(json!({ "untrusted": true, "private": true }))));
            }
            tokio::time::sleep(CHILD_POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(child: &str, parent: &str, tools: &[&str]) -> SubAgent {
        SubAgent { child: child.into(), parent: parent.into(), tools: tools.iter().map(|t| t.to_string()).collect() }
    }

    #[test]
    fn a_limit_above_binds_every_child_below_it() {
        let mut l = Ledger::default();
        l.apply(&s("b", "a", &["read"]));
        l.apply(&s("c", "b", &[]));
        assert!(l.refusal("c", "send").is_some(), "a grandchild escaped its parent's limit");
        assert!(l.refusal("c", "read").is_none());
        assert!(l.refusal("a", "send").is_none(), "the top run has no limit");
    }

    #[test]
    fn a_share_of_the_budget_never_exceeds_half() {
        let b = child_budget(&Budget { max_model_calls: 100, max_actions: 40, max_usd: 2.0 }, Some(0.9));
        assert_eq!((b.max_model_calls, b.max_actions), (50, 20));
        assert!((b.max_usd - 1.0).abs() < f64::EPSILON);
    }
}
