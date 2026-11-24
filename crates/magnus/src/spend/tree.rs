//! A sub-agent tree shares its limits (decisions/2026-11-23-a-sub-agent-tree-shares-its-limits.md).
//!
//! A sub-agent spends its parent's budget. Its dollars were always counted
//! up the tree (`Run::sub_usd`); its actions and model calls are counted
//! the same way here. A run's limits bind the run and every sub-agent below
//! it together, so children that each keep within their own share cannot
//! together pass their parent's limits, nor those of any run above it.

use super::allowance::{Left, Spender, Stop};
use super::holds::Hold;
use crate::state::{Run, State};
use serde_json::json;

/// What a run and every sub-agent below it have used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Used {
    pub actions: u32,
    /// The agent loops' own model calls (`Run::loop_calls`).
    pub steps: u32,
    /// Every model call, the kernel's extras included.
    pub model_calls: u32,
}

/// The totals of `run` and every sub-agent below it.
pub fn used(s: &State, run: &str) -> Used {
    let below = s.sub_agents.below(run).into_iter().map(|(_, a)| a.child);
    std::iter::once(run.to_string()).chain(below).filter_map(|r| s.runs.get(&r)).fold(Used::default(), |u, r| Used {
        actions: u.actions.saturating_add(r.actions),
        steps: u.steps.saturating_add(r.loop_calls()),
        model_calls: u.model_calls.saturating_add(r.model_calls),
    })
}

/// Why `runs[0]` may not spend once more for `spender`: a limit of its own
/// or of a run above it (`runs`, the run first, then its parents), each
/// counted over that run's whole tree. An action counts too what another
/// run's batch holds there (`holds::Holds::reserve`).
pub(super) fn stop(s: &State, spender: Spender, runs: &[String], held: &[Hold]) -> Option<Box<Stop>> {
    let asker = runs.first().map(String::as_str).unwrap_or_default();
    runs.iter().enumerate().find_map(|(i, id)| {
        let run = s.runs.get(id)?;
        let mut used = used(s, id);
        if spender == Spender::Action {
            used.actions = used.actions.saturating_add(held_actions(s, held, id, asker));
        }
        stop_for(run, used, spender, i > 0)
    })
}

/// How many actions `runs[0]` may still take: the least any run in `runs`
/// (the run, then its parents) has left, each counted over its whole tree
/// with what other runs' batches hold there.
pub fn actions_left(s: &State, runs: &[String], held: &[Hold]) -> u32 {
    let asker = runs.first().map(String::as_str).unwrap_or_default();
    runs.iter()
        .filter_map(|id| Some((s.runs.get(id)?, id)))
        .map(|(run, id)| run.config.budget.max_actions.saturating_sub(used(s, id).actions.saturating_add(held_actions(s, held, id, asker))))
        .min()
        .unwrap_or(0)
}

/// The actions held against `id`'s tree by batches of runs other than
/// `asker`: a batch's actions are its own calls' to take.
fn held_actions(s: &State, held: &[Hold], id: &str, asker: &str) -> u32 {
    held.iter()
        .filter(|h| h.actions > 0 && h.runs.first().map(String::as_str) != Some(asker) && h.runs.iter().any(|r| r == id))
        .map(|h| still_held(s, h))
        .fold(0, u32::saturating_add)
}

/// What a batch still holds: the actions its run has not yet recorded.
fn still_held(s: &State, h: &Hold) -> u32 {
    let recorded = h.runs.first().and_then(|r| s.runs.get(r)).map_or(0, |r| r.actions.saturating_sub(h.actions_at));
    h.actions.saturating_sub(recorded)
}

fn stop_for(run: &Run, used: Used, spender: Spender, above: bool) -> Option<Box<Stop>> {
    let b = &run.config.budget;
    // Said of the run itself as before; of a run above, as that task's.
    let theirs = |what: String| format!("the task this sub-agent works for has used {what}, its sub-agents' included");
    let calls = match spender {
        Spender::Step => Some(used.steps),
        Spender::Extra => Some(used.model_calls),
        Spender::Action | Spender::Check | Spender::Made => None,
    };
    if let Some(n) = calls.filter(|n| *n >= b.max_model_calls) {
        let what = format!("{n} of {} model calls", b.max_model_calls);
        let (why, notice) = if above {
            let why = theirs(what);
            (why.clone(), format!("Stopped: {why}."))
        } else {
            (what, format!("Stopped after {n} model calls, the task's limit."))
        };
        return Some(Box::new(Stop { reason: "budget", why, notice, details: json!({ "model_calls": n, "limit": b.max_model_calls, "usd": run.spent_usd() }) }));
    }
    // Dollars are spent by model calls, failed ones too: one past the limit
    // stops the run before the next is paid for.
    let actions_spent = spender == Spender::Action && used.actions >= b.max_actions;
    if actions_spent || run.spent_usd() >= b.max_usd {
        let what = format!("{} of {} actions, ${:.2} of ${:.2}", used.actions, b.max_actions, run.spent_usd(), b.max_usd);
        let why = if above { theirs(what) } else { format!("budget used: {what}") };
        let notice = format!("Stopped: {why}.");
        return Some(Box::new(Stop { reason: "budget", why, notice, details: json!({ "actions": used.actions, "usd": run.spent_usd() }) }));
    }
    None
}

/// The dollars left of each run's budget in `runs`, less what the calls in
/// flight below it hold.
pub(super) fn left(s: &State, runs: &[String], held: &[Hold], left: Left) -> Left {
    runs.iter().filter_map(|id| s.runs.get(id)).fold(left, |left, run| {
        let held: f64 = held.iter().filter(|h| h.runs.contains(&run.id)).map(|h| h.usd).sum();
        left.under(Some(run.config.budget.max_usd - run.spent_usd() - held), "task")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spend::allowance::allowance;
    use crate::sub_agents::SubAgent;
    use chrono::Utc;
    use serde_json::Value;

    fn ev(s: &mut State, seq: i64, actor: &str, e: Value) {
        s.apply(&crate::log::Event { seq, at: Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
    }

    fn created(s: &mut State, seq: i64, task: &str, run: &str, calls: u32, actions: u32) {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": calls, "max_actions": actions, "max_usd": 10.0 }, "verifier": false, "instructions": "", "harness": "" });
        ev(s, seq, "person:ada", json!({ "kind": "task_created", "task": task, "run": run, "person": "ada", "text": "x", "config": config }));
    }

    fn child(s: &mut State, seq: i64, run: &str, parent: &str, calls: u32, actions: u32) {
        s.sub_agents.apply(&SubAgent { child: run.into(), parent: parent.into(), tools: vec![] });
        created(s, seq, &format!("t-{run}"), run, calls, actions);
    }

    /// A top run with a 40-action, 10-call limit, and two children with
    /// 20 actions and 5 calls each, the most a share gives.
    fn tree() -> State {
        let mut s = State::default();
        created(&mut s, 1, "top", "rt", 10, 40);
        child(&mut s, 2, "r1", "rt", 5, 20);
        child(&mut s, 3, "r2", "rt", 5, 20);
        s
    }

    fn set(s: &mut State, run: &str, calls: u32, actions: u32) {
        let r = s.runs.get_mut(run).unwrap();
        (r.model_calls, r.actions) = (calls, actions);
    }

    /// Children that each keep within their own limits but together pass
    /// the top's are stopped, and so is the top itself.
    #[test]
    fn children_within_their_own_limits_are_stopped_at_the_tops() {
        let mut s = tree();
        set(&mut s, "rt", 1, 1);
        set(&mut s, "r1", 4, 19);
        set(&mut s, "r2", 4, 19);
        assert_eq!(used(&s, "rt"), Used { actions: 39, steps: 9, model_calls: 9 });
        assert!(allowance(&s, "r1", Spender::Action, Utc::now()).is_ok(), "one action is left of the top's 40");
        set(&mut s, "rt", 1, 2);
        let stop = allowance(&s, "r1", Spender::Action, Utc::now()).expect_err("a child passed the top's action limit with its sibling's");
        assert!(stop.why.starts_with("the task this sub-agent works for has used 40 of 40 actions"), "{}", stop.why);
        assert!(allowance(&s, "rt", Spender::Action, Utc::now()).is_err(), "the top acted past its limit after its children");
        // Model calls the same way: 2 + 4 + 4 reaches the top's 10.
        assert!(allowance(&s, "r1", Spender::Step, Utc::now()).is_ok());
        set(&mut s, "rt", 2, 2);
        let stop = allowance(&s, "r1", Spender::Step, Utc::now()).expect_err("a child called the model past the top's limit");
        assert_eq!(stop.details["limit"], 10, "{}", stop.why);
        assert!(allowance(&s, "r1", Spender::Check, Utc::now()).is_ok(), "a safety check is bounded by dollars alone");
    }

    /// A grandchild is held to every run above it, not just its parent.
    #[test]
    fn a_grandchild_is_held_to_the_top() {
        let mut s = tree();
        child(&mut s, 4, "r3", "r1", 2, 10);
        set(&mut s, "rt", 0, 30);
        set(&mut s, "r2", 0, 9);
        assert!(allowance(&s, "r3", Spender::Action, Utc::now()).is_ok());
        set(&mut s, "r3", 0, 1);
        assert!(allowance(&s, "r3", Spender::Action, Utc::now()).is_err(), "a grandchild passed the top's limit");
        assert!(allowance(&s, "r2", Spender::Step, Utc::now()).is_ok(), "an action limit stopped a model call");
    }

    /// The dollars a run's budget has left count what is held for calls in
    /// flight anywhere below a run above it.
    #[test]
    fn the_dollars_left_are_the_least_of_every_run_above() {
        let mut s = tree();
        s.runs.get_mut("rt").unwrap().sub_usd = 9.0;
        let runs = vec!["r1".to_string(), "rt".to_string()];
        let l = left(&s, &runs, &[Hold { runs: vec!["r2".into(), "rt".into()], usd: 0.5, ..Hold::default() }], Left { usd: f64::INFINITY, of: "" });
        assert!((l.usd - 0.5).abs() < 1e-9, "{}", l.usd);
    }

    /// The actions a batch of the top's holds are its own calls' to take: a
    /// child cannot take them first, and each the top records frees one.
    #[test]
    fn a_batchs_held_actions_are_not_a_childs_to_take() {
        use crate::spend::allowance::allowance_held;
        let mut s = tree();
        set(&mut s, "rt", 0, 30);
        set(&mut s, "r1", 0, 4);
        let held = [Hold { runs: vec!["rt".into()], actions: 5, actions_at: 30, ..Hold::default() }];
        let rt = ["rt".to_string()];
        assert_eq!(actions_left(&s, &rt, &held), 6, "the top's own batch was counted against it");
        let r1 = ["r1".to_string(), "rt".to_string()];
        assert_eq!(actions_left(&s, &r1, &held), 1, "a child was not held to what its parent's batch holds");
        assert!(allowance_held(&s, "r1", Spender::Action, Utc::now(), &held).is_ok());
        set(&mut s, "r1", 0, 5);
        assert!(allowance_held(&s, "r1", Spender::Action, Utc::now(), &held).is_err(), "a child took an action its parent's batch holds");
        assert!(allowance_held(&s, "rt", Spender::Action, Utc::now(), &held).is_ok(), "the top's batch stopped its own calls");
        // The top's calls record two of the five: three are still held.
        set(&mut s, "rt", 0, 32);
        set(&mut s, "r1", 0, 4);
        assert_eq!(actions_left(&s, &r1, &held), 1);
        assert_eq!(actions_left(&s, &rt, &held), 4);
    }
}
