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
    /// The children in the order they were started.
    started: Vec<String>,
}

impl Ledger {
    pub fn apply(&mut self, s: &SubAgent) {
        if self.children.insert(s.child.clone(), s.clone()).is_none() {
            self.started.push(s.child.clone());
        }
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

    /// The run at the top of a sub-agent's parents; None for a run that is
    /// no sub-agent.
    pub fn top(&self, run: &str) -> Option<String> {
        let mut at = &self.children.get(run)?.parent;
        for _ in 0..MAX_DEPTH {
            match self.children.get(at) {
                Some(s) => at = &s.parent,
                None => break,
            }
        }
        Some(at.clone())
    }

    /// Every sub-agent below a run, each after its parent, with its depth
    /// (1 for the run's own), in the order they were started.
    pub fn below(&self, run: &str) -> Vec<(usize, SubAgent)> {
        let mut out = vec![];
        let mut stack: Vec<(usize, &str)> = vec![(0, run)];
        while let Some((depth, at)) = stack.pop() {
            if depth > MAX_DEPTH {
                continue;
            }
            let kids: Vec<&SubAgent> = self.started.iter().filter_map(|c| self.children.get(c)).filter(|s| s.parent == at).collect();
            for s in kids.iter().rev() {
                stack.push((depth + 1, &s.child));
            }
            if depth > 0 {
                if let Some(s) = self.children.get(at) {
                    out.push((depth, s.clone()));
                }
            }
        }
        out
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

/// A refusal is the kernel's own sentence: no sub-agent ran, so nothing was
/// read and the parent takes on no taint.
fn refused(text: String) -> (String, bool, Option<Value>) {
    (text, true, Some(json!({ "untrusted": false, "private": false })))
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
            return Ok(refused("Sub-agents cannot be nested this deeply.".to_string()));
        }
        let parent_grant = self.read(|s| s.tasks.get(&parent.task).and_then(|t| t.grant.clone()));
        let grant = a.input["grant"].as_str().map(String::from);
        if grant.is_some() && grant != parent_grant {
            return Ok(refused("That grant is not one this run holds, so a sub-agent cannot be given it.".to_string()));
        }
        let mut tools: Vec<String> = a.input["tools"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
        // A child never gets to start more than its own limit lets it.
        let ceiling = self.read(|s| s.sub_agents.get(&a.run).map(|p| p.tools.clone())).unwrap_or_default();
        if !ceiling.is_empty() {
            if let Some(extra) = tools.iter().find(|t| !ceiling.contains(t)) {
                return Ok(refused(format!("{extra} is not a tool this sub-agent holds.")));
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
    fn below_lists_each_child_after_its_parent_in_start_order() {
        let mut l = Ledger::default();
        l.apply(&s("z", "turn", &[]));
        l.apply(&s("a", "turn", &[]));
        l.apply(&s("m", "z", &[]));
        let order: Vec<(usize, String)> = l.below("turn").into_iter().map(|(d, s)| (d, s.child)).collect();
        assert_eq!(order, [(1, "z".to_string()), (2, "m".to_string()), (1, "a".to_string())]);
        assert_eq!(l.top("m").as_deref(), Some("turn"), "a grandchild lost its chat turn");
        assert_eq!(l.top("turn"), None, "the top run counted as a sub-agent");
    }

    #[test]
    fn a_share_of_the_budget_never_exceeds_half() {
        let b = child_budget(&Budget { max_model_calls: 100, max_actions: 40, max_usd: 2.0 }, Some(0.9));
        assert_eq!((b.max_model_calls, b.max_actions), (50, 20));
        assert!((b.max_usd - 1.0).abs() < f64::EPSILON);
    }
}
