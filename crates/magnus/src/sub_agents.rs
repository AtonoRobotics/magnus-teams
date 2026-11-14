//! Sub-agents: child runs with their own context, a goal and a share of the
//! parent's budget. A child carries its parent's authority (owner,
//! 2026-11-10): the grant, the confirmed scope (a schedule's too), the
//! project, the requester and the approvals given to the parent's task,
//! looked up through its parents wherever they are checked. It needs no
//! grant of its own; the parent only narrows it, by naming a subset of its
//! own tools. Its spend is the parent's, and its reading taints the parent.
use crate::boundary::Action;
use crate::config::Budget;
use crate::kernel::{Kernel, NewRun};
use crate::state::{Run, State, Task};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
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

/// The sub-agents of their runs.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.children.retain(|child, s| !who.runs.contains(child) && !who.runs.contains(&s.parent));
        let children = &self.children;
        self.started.retain(|c| children.contains_key(c));
    }
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

/// A share of what is left of the parent's budget, however it was asked
/// for: the child's spend is the parent's too (`Run::spent_usd`).
fn child_budget(parent: &Budget, spent_usd: f64, share: Option<f64>) -> Budget {
    let share = share.unwrap_or(DEFAULT_CHILD_SHARE).clamp(0.0, MAX_CHILD_SHARE);
    Budget {
        max_model_calls: ((f64::from(parent.max_model_calls) * share) as u32).max(1),
        max_actions: ((f64::from(parent.max_actions) * share) as u32).max(1),
        max_usd: (parent.max_usd - spent_usd).max(0.0) * share,
    }
}

/// Tools retired in spec rev 147: a sub-agent's list that names one still starts.
const RETIRED_TOOLS: &[&str] = &["task_contract", "propose_grant"];

/// The tools a sub-agent is limited to: the ones the parent named, each one
/// the parent holds and within its own limit; none named, the parent's own
/// limit. `held` is every tool the parent may use; an empty `ceiling` is no
/// limit of its own.
fn narrowed(mut named: Vec<String>, held: &[String], ceiling: Vec<String>) -> std::result::Result<Vec<String>, String> {
    // A retired tool names nothing to hold (spec rev 147): it is dropped,
    // not refused, so a call written before it was retired still starts.
    named.retain(|t| !RETIRED_TOOLS.contains(&t.as_str()));
    if let Some(extra) = named.iter().find(|t| !held.contains(t) || (!ceiling.is_empty() && !ceiling.contains(t))) {
        return Err(format!("{extra} is not a tool you hold, so a sub-agent cannot be given it: it works within your authority, never more."));
    }
    Ok(if named.is_empty() { ceiling } else { named })
}

/// What `start_sub_agent` takes. A call written before 2026-11-10 may carry
/// `grant`: that field is retired and read as nothing, since a sub-agent
/// always works under its parent's grant and no other.
#[derive(Deserialize)]
struct Ask {
    goal: String,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    budget_share: Option<f64>,
}

impl Run {
    /// What the run has spent against its budget: its own calls and its
    /// sub-agents'.
    pub fn spent_usd(&self) -> f64 {
        self.usd + self.sub_usd
    }
}

impl State {
    /// The task whose authority a run acts under: the top run's, itself
    /// when it is no sub-agent. A sub-agent carries its grant, confirmed
    /// scope (a schedule's too), project and requester, and is background
    /// work when it is.
    pub fn authority(&self, run: &str) -> Option<&Task> {
        let top = self.sub_agents.top(run).unwrap_or_else(|| run.to_string());
        self.runs.get(&top).and_then(|r| self.tasks.get(&r.task))
    }

    /// A run's task as its authority stands: its own, with the grant,
    /// schedule and project of the run at the top of its parents.
    pub fn with_authority(&self, run: &str) -> Option<Task> {
        let mut t = self.runs.get(run).and_then(|r| self.tasks.get(&r.task)).cloned()?;
        let top = self.authority(run);
        t.grant = top.and_then(|a| a.grant.clone());
        t.schedule = top.and_then(|a| a.schedule.clone());
        t.project = top.and_then(|a| a.project.clone());
        Some(t)
    }

    /// The tasks whose approvals hold for a run: its own, then each
    /// parent's. An approval the parent got covers the same action here.
    pub fn lineage(&self, run: &str) -> Vec<String> {
        let mut out = vec![];
        let mut at = run.to_string();
        for _ in 0..=MAX_DEPTH {
            out.extend(self.runs.get(&at).map(|r| r.task.clone()));
            match self.sub_agents.get(&at) {
                Some(s) => at = s.parent.clone(),
                None => break,
            }
        }
        out
    }

    /// A model call's cost: charged to the grant the run acts under (#280),
    /// and to every run above it, whose budget it spends too.
    pub(crate) fn charge(&mut self, run: &str, at: DateTime<Utc>, usd: f64) {
        let grant = self.authority(run).and_then(|t| t.grant.clone());
        if let Some(g) = grant.and_then(|g| self.grants.get_mut(&g)) {
            g.charges.push((at, usd));
        }
        let mut below = run.to_string();
        for _ in 0..MAX_DEPTH {
            let Some(parent) = self.sub_agents.get(&below).map(|s| s.parent.clone()) else { break };
            if let Some(r) = self.runs.get_mut(&parent) {
                r.sub_usd += usd;
            }
            below = parent;
        }
    }

    /// The run whose person's words a run acts on, for its requester: the
    /// top of its parents. A sub-agent's goal is its parent model's words,
    /// never the person's; one whose top run is gone has no one's words.
    pub fn requester_run(&self, run: &Run) -> Run {
        match self.sub_agents.top(&run.id) {
            None => Some(run.clone()),
            Some(top) => self.runs.get(&top).cloned(),
        }
        .unwrap_or_else(|| Run { task: String::new(), ..run.clone() })
    }
}

impl Kernel {
    /// `State::requester_run`.
    pub(crate) fn requester_run(&self, run: &Run) -> Run {
        self.read(|s| s.requester_run(run))
    }

    pub(crate) async fn start_sub_agent(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let parent = self.read(|s| s.runs.get(&a.run).cloned()).context("no such run")?;
        let ask: Ask = serde_json::from_value(a.input.clone()).context("goal is required")?;
        if self.read(|s| s.sub_agents.depth(&a.run)) >= MAX_DEPTH {
            return Ok(refused("Sub-agents cannot be nested this deeply.".to_string()));
        }
        // Never a tool the parent lacks, nor one outside its own limit.
        let held: Vec<String> = self.every_tool_for(&parent).await.into_iter().map(|t| t.name).collect();
        let ceiling = self.read(|s| s.sub_agents.get(&a.run).map(|p| p.tools.clone())).unwrap_or_default();
        let tools = match narrowed(ask.tools, &held, ceiling) {
            Ok(t) => t,
            Err(why) => return Ok(refused(why)),
        };
        // The scope the person confirmed for the top run is said to the
        // child as it was to the parent. Its grant, schedule and project
        // are looked up through its parents wherever they are checked.
        let top = self.read(|s| s.authority(&a.run).cloned());
        let scope = match &top {
            Some(t) => self.words_of(&t.person, t.seq).await?.opt(t.seq, "scope"),
            None => None,
        };
        // What the parent has read comes along in the goal it writes.
        let taint = crate::tools::Label { untrusted: parent.untrusted, private: parent.private };
        let (task, child) = (crate::kernel::new_id("task"), crate::kernel::new_id("run"));
        let o = NewRun {
            label: (taint.untrusted || taint.private).then_some(taint),
            budget: Some(child_budget(&parent.config.budget, parent.spent_usd(), ask.budget_share)),
            model: Some((parent.config.adapter.clone(), parent.config.model.clone())),
            ids: Some((task.clone(), child.clone())),
            scope,
            // A sub-agent of a team run is a team run from its first call.
            team: self.team_of(&parent.task).map(|t| crate::team::TeamRun { sub: true, ..t }),
            ..Default::default()
        };
        // Recorded before the child starts: its first call already acts
        // within the parent's authority and its own limits.
        self.emit("kernel", crate::events::Ev::SubAgent(SubAgent { child: child.clone(), parent: a.run.clone(), tools })).await?;
        self.create_run(&format!("run:{}", a.run), &parent.person, ask.goal, vec![], o).await?;
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

    fn ev(s: &mut State, seq: i64, actor: &str, e: Value) {
        s.apply(&crate::log::Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
    }

    fn created(s: &mut State, seq: i64, actor: &str, task: &str, run: &str, more: Value) {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
        let mut e = json!({ "kind": "task_created", "task": task, "run": run, "person": "alice", "text": "x", "config": config });
        for (k, v) in more.as_object().cloned().unwrap_or_default() {
            e[k] = v;
        }
        ev(s, seq, actor, e);
    }

    /// A scheduled run under a grant, in a project, with a sub-agent and
    /// that one's own: the sub-agents were started with no grant, project or
    /// schedule of their own.
    fn scheduled_run_with_sub_agents() -> State {
        let mut s = State::default();
        ev(&mut s, 1, "person:alice", json!({ "kind": "grant_set", "id": "g1", "person": "alice", "apps": ["notes"], "period": "week", "max_usd": 5.0, "cedar": "" }));
        ev(
            &mut s,
            2,
            "person:alice",
            json!({ "kind": "schedule_set", "id": "sc1", "person": "alice", "daily_at": "08:00", "scope": "Works on the notes; sends nothing.", "thread": "th" }),
        );
        let project = json!({ "id": "p1", "name": "Launch", "files": ["f1"] });
        created(&mut s, 3, "schedule:sc1", "top", "rt", json!({ "grant": "g1", "project": project, "scope": "Works on the notes; sends nothing." }));
        ev(&mut s, 4, "kernel", json!({ "kind": "schedule_fired", "id": "sc1", "task": "top" }));
        ev(&mut s, 5, "kernel", json!({ "kind": "sub_agent", "child": "r1", "parent": "rt", "tools": [] }));
        created(&mut s, 6, "run:rt", "c1", "r1", json!({}));
        ev(&mut s, 7, "kernel", json!({ "kind": "sub_agent", "child": "r2", "parent": "r1", "tools": ["notes__list"] }));
        created(&mut s, 8, "run:r1", "c2", "r2", json!({}));
        s
    }

    /// Every part of the parent's authority reaches the sub-agent below it,
    /// found through its parents: the grant, the schedule whose confirmed
    /// scope it keeps to, the project, the requester and the tasks whose
    /// approvals hold for it.
    #[test]
    fn a_sub_agent_carries_its_parents_authority() {
        let s = scheduled_run_with_sub_agents();
        assert_eq!(s.tasks["c2"].grant, None, "the sub-agent was given a grant of its own");
        let t = s.with_authority("r2").expect("no task");
        assert_eq!(t.task_id, "c2", "the sub-agent's own task, with its own goal");
        assert_eq!(t.grant.as_deref(), Some("g1"), "the sub-agent lost its parent's grant");
        assert_eq!(t.schedule.as_deref(), Some("sc1"), "the sub-agent lost the schedule its scope was confirmed for");
        assert_eq!(t.project.as_ref().map(|p| p.id.as_str()), Some("p1"), "the sub-agent left its parent's project");
        assert_eq!(s.authority("r2").map(|t| t.task_id.as_str()), Some("top"));
        assert_eq!(s.requester_run(&s.runs["r2"]).id, "rt", "the sub-agent acts on its parent's words, not the person's");
        assert_eq!(s.lineage("r2"), ["c2", "c1", "top"], "an approval given to a parent's task does not hold for its sub-agent");
        // A run that is no sub-agent stands on its own.
        assert_eq!(s.lineage("rt"), ["top"]);
        assert_eq!(s.requester_run(&s.runs["rt"]).id, "rt");
    }

    /// A sub-agent whose top run is gone has no one's words to act on.
    #[test]
    fn a_sub_agent_whose_parent_is_gone_has_no_requester() {
        let mut s = State::default();
        ev(&mut s, 1, "kernel", json!({ "kind": "sub_agent", "child": "r1", "parent": "gone", "tools": [] }));
        created(&mut s, 2, "run:gone", "c1", "r1", json!({}));
        assert_eq!(s.requester_run(&s.runs["r1"]).task, "", "a sub-agent's goal was read as its person's words");
        assert_eq!(s.with_authority("r1").and_then(|t| t.grant), None);
    }

    /// What a sub-agent spends is its parent's: charged to the parent's
    /// grant and counted against every budget above it.
    #[test]
    fn a_sub_agents_spend_counts_against_its_parents_grant_and_budget() {
        let mut s = scheduled_run_with_sub_agents();
        let usage = json!({ "input_tokens": 1, "output_tokens": 1, "cost_usd": 0.75 });
        ev(&mut s, 9, "model", json!({ "kind": "model_reply", "run": "r2", "step": 1, "usage": usage, "served_model": "m", "stop": "end_turn" }));
        assert_eq!(s.grants["g1"].charges.iter().map(|(_, usd)| *usd).sum::<f64>(), 0.75, "the sub-agent's call was not charged to its parent's grant");
        assert_eq!((s.runs["rt"].sub_usd, s.runs["r1"].sub_usd, s.runs["r2"].sub_usd), (0.75, 0.75, 0.0));
        assert_eq!((s.runs["rt"].spent_usd(), s.runs["r2"].spent_usd()), (0.75, 0.75));
        assert_eq!(s.runs["rt"].usd, 0.0, "the parent's own calls now hold its sub-agent's");
    }

    /// The parent narrows a sub-agent by naming some of its own tools, and
    /// can never widen it: no tool it lacks, and none past its own limit.
    #[test]
    fn a_parent_only_narrows_a_sub_agent_by_its_own_tools() {
        let held: Vec<String> = ["notes__list", "notes__save", "sandbox_exec"].iter().map(|t| t.to_string()).collect();
        let v = |t: &[&str]| t.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        assert_eq!(narrowed(v(&["notes__list"]), &held, vec![]), Ok(v(&["notes__list"])));
        assert_eq!(narrowed(vec![], &held, vec![]), Ok(vec![]), "none named is the parent's own");
        assert_eq!(narrowed(vec![], &held, v(&["notes__list"])), Ok(v(&["notes__list"])), "a limited parent's child escaped its limit");
        assert!(narrowed(v(&["crm__update_record"]), &held, vec![]).is_err(), "a sub-agent was given a tool its parent lacks");
        assert!(narrowed(v(&["notes__save"]), &held, v(&["notes__list"])).is_err(), "a sub-agent was given a tool past its parent's limit");
        assert_eq!(narrowed(v(&["task_contract", "notes__list"]), &held, vec![]), Ok(v(&["notes__list"])), "a retired tool stopped a sub-agent");
        // And at the boundary, every limit above binds it.
        let s = scheduled_run_with_sub_agents();
        assert!(s.sub_agents.refusal("r2", "notes__save").is_some());
        assert!(s.sub_agents.refusal("r2", "notes__list").is_none());
    }

    /// A call written before the grant input was retired still starts a
    /// sub-agent: the field is read as nothing.
    #[test]
    fn a_retired_grant_input_is_read_as_nothing() {
        let a: Ask = serde_json::from_value(json!({ "goal": "Check the notes.", "grant": "grant_someone_elses", "tools": ["notes__list"] })).unwrap();
        assert_eq!((a.goal.as_str(), a.tools.as_slice(), a.budget_share), ("Check the notes.", &["notes__list".to_string()][..], None));
        let spec = crate::tools_identity::sub_agent_spec(&|_| String::new());
        assert!(spec.input_schema["properties"].get("grant").is_none(), "start_sub_agent still takes a grant");
    }

    #[test]
    fn a_share_of_the_budget_never_exceeds_half() {
        let b = child_budget(&Budget { max_model_calls: 100, max_actions: 40, max_usd: 2.0 }, 0.0, Some(0.9));
        assert_eq!((b.max_model_calls, b.max_actions), (50, 20));
        assert!((b.max_usd - 1.0).abs() < f64::EPSILON);
        // Of what is left: the parent's spend so far, its sub-agents' too, is gone.
        let b = child_budget(&Budget { max_model_calls: 100, max_actions: 40, max_usd: 2.0 }, 1.5, Some(0.5));
        assert!((b.max_usd - 0.25).abs() < f64::EPSILON, "a sub-agent got more than its parent has left: {}", b.max_usd);
        assert!(child_budget(&Budget { max_model_calls: 1, max_actions: 1, max_usd: 2.0 }, 3.0, None).max_usd == 0.0);
    }
}
