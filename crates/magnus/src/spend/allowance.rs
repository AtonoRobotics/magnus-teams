//! A run's allowance: whether it may spend more, and how much.
//!
//! Every paid step of a run asks here once: the agent loop before each
//! model call, the boundary before each action the model asked for, and
//! the kernel's optional extra calls (reviewer, memory pass, knowledge
//! extraction, spoken script) before theirs. One place holds every rule:
//!
//! - the run's own budget: its model calls, its actions and its dollars,
//!   counted with its sub-agents', and the budget of every run above it;
//! - its grant's budget for the period (#280, #286);
//! - the month's limits, the person's and the server's (2026-11-10);
//! - its team's, the workspace's and the channel's (2026-11-13).
//!
//! The limits hold partway through a run for every spender alike. Only the
//! call cap differs by spender: the agent's steps are counted apart from the
//! kernel's extras, which never use them up but skip themselves once the
//! run's total calls reach the cap (2026-11-09). Actions are bounded by the
//! action cap instead, and the safety checks on the run's work by dollars
//! alone.
//!
//! What is left is the least of what every limit has left, less what calls
//! in flight hold against it (`holds`): the run's budget, its grant's, the
//! person's month and the server's, its workspace's and its channel's. A
//! call's worst case must fit that, so no limit is passed by one call, nor
//! by two at once (2026-11-23).

use super::holds::{held_for, Hold};
use crate::kernel::Kernel;
use crate::state::State;
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
    /// A safety check on the run's work (the monitor, a screen): bounded
    /// by dollars, never by the call cap, so the run's last step is still
    /// checked.
    Check,
    /// What an action the model asked for has a provider make (a
    /// picture): the action was counted already, so it is bounded by
    /// dollars alone.
    Made,
}

/// What may still be spent.
#[derive(Debug, Clone, PartialEq)]
pub struct Left {
    /// The dollars left under every limit, which a call's worst case must
    /// fit; infinite when none applies.
    pub usd: f64,
    /// The limit with least left: `task`, `grant`, `person`, `server`,
    /// `workspace` or `channel`.
    pub of: &'static str,
}

impl Left {
    fn none() -> Left {
        Left { usd: f64::INFINITY, of: "" }
    }

    /// The tighter of this and `usd` left of `of`.
    pub(super) fn under(self, usd: Option<f64>, of: &'static str) -> Left {
        match usd {
            Some(u) if u < self.usd => Left { usd: u.max(0.0), of },
            _ => self,
        }
    }

    /// What the person reads of the limit a call would pass.
    pub fn named(&self) -> &'static str {
        match self.of {
            "grant" => "the grant's budget",
            "person" => "your spend limit for this month",
            "server" => "the server's spend limit for this month",
            "workspace" => "the workspace's spend limit for this month",
            "channel" => "the channel's spend limit for this month",
            _ => "the task's budget",
        }
    }
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

/// Whether `run` may spend once more for `spender` at `now`, and what it
/// has left; a run that is not there has nothing.
pub fn allowance(s: &State, run_id: &str, spender: Spender, now: DateTime<Utc>) -> Result<Left, Box<Stop>> {
    allowance_held(s, run_id, spender, now, &[])
}

/// The same, with the calls in flight holding what they may cost.
pub fn allowance_held(s: &State, run_id: &str, spender: Spender, now: DateTime<Utc>, held: &[Hold]) -> Result<Left, Box<Stop>> {
    let Some(run) = s.runs.get(run_id) else {
        return Err(Box::new(Stop { reason: "budget", why: "the run is not there".into(), notice: String::new(), details: Value::Null }));
    };
    // The run's own budget, and every run's above it, each counted over
    // its whole tree of sub-agents (`tree`).
    let scope = scope_of_run(s, run_id);
    if let Some(stop) = super::tree::stop(s, spender, &scope.runs, held) {
        return Err(stop);
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
    let h = held_for(held, &scope);
    let left = super::tree::left(s, &scope.runs, held, Left::none());
    let left = left.under(crate::grants::left_for_run(s, run_id, now).map(|l| l - h.grant), "grant");
    let left = month_left(s, &run.person, now, h.person, h.server, left);
    Ok(team_left(s, scope.workspace.as_deref(), scope.channel.as_deref(), now, h.workspace, h.channel, left))
}

/// The person's month and the server's, less what is held against them.
fn month_left(s: &State, person: &str, now: DateTime<Utc>, held_person: f64, held_server: f64, left: Left) -> Left {
    let m = super::month::key(now);
    let (all, theirs) = super::month::spent_by(s, &m, person);
    let theirs_left = (!person.is_empty()).then(|| s.spend.person_limit(person, &m)).flatten().map(|l| l - theirs - held_person);
    left.under(s.spend.server_usd.map(|l| l - all - held_server), "server").under(theirs_left, "person")
}

/// A workspace's month and its channel's, less what is held against them.
#[allow(clippy::too_many_arguments)]
fn team_left(s: &State, ws: Option<&str>, channel: Option<&str>, now: DateTime<Utc>, held_ws: f64, held_channel: f64, left: Left) -> Left {
    let Some(w) = ws.and_then(|ws| s.team.workspace(ws)) else { return left };
    let spent = crate::team::spend::spent_this_month(s, &w.id, now);
    let whole = w.chat.limit_usd.map(|l| l - spent.total_usd - held_ws);
    let here = channel.and_then(|c| Some(w.chat.channel_limits_usd.get(c)? - spent.channels_usd.get(c).copied().unwrap_or_default() - held_channel));
    left.under(whole, "workspace").under(here, "channel")
}

/// The limits a run's calls count against, as a hold names them: the run
/// and those above it, the grant it acts under, its person, and its team's
/// workspace and channel.
pub fn scope_of_run(s: &State, run: &str) -> Hold {
    let mut runs = vec![run.to_string()];
    let mut at = run.to_string();
    for _ in 0..crate::sub_agents::MAX_DEPTH {
        let Some(parent) = s.sub_agents.get(&at).map(|x| x.parent.clone()) else { break };
        runs.push(parent.clone());
        at = parent;
    }
    let team = s.authority(run).and_then(|t| s.team.run_of(&t.task_id));
    Hold {
        runs,
        grant: crate::grants::binding(s, run).map(|g| g.id.clone()),
        person: s.runs.get(run).map(|r| r.person.clone()),
        workspace: team.map(|t| t.workspace.clone()),
        channel: team.map(|t| t.channel.clone()),
        ..Hold::default()
    }
}

/// Who pays for a call made for no run.
#[derive(Debug, Clone, PartialEq)]
pub enum Owner {
    Person(String),
    Workspace(String),
    /// The server's own: a tool definition's screen outside any run.
    Server,
}

impl Owner {
    /// As a `charged` change names it.
    pub fn key(&self) -> String {
        match self {
            Owner::Person(p) => format!("person:{p}"),
            Owner::Workspace(w) => format!("workspace:{w}"),
            Owner::Server => super::book::SERVER_AGENT.to_string(),
        }
    }

    /// From what a computer's owner or an egress names: `workspace:<id>`
    /// or a person's id.
    pub fn of(who: &str) -> Owner {
        match who.strip_prefix("workspace:") {
            Some(w) => Owner::Workspace(w.to_string()),
            None => Owner::Person(who.to_string()),
        }
    }

    /// The limits its calls count against.
    pub fn scope(&self) -> Hold {
        match self {
            Owner::Person(p) => Hold { person: Some(p.clone()), ..Hold::default() },
            Owner::Workspace(w) => Hold { workspace: Some(w.clone()), ..Hold::default() },
            Owner::Server => Hold::default(),
        }
    }
}

/// What an owner may still spend this month: a person's month and the
/// server's; a workspace's month and the server's; the server's.
pub fn owner_allowance(s: &State, owner: &Owner, now: DateTime<Utc>, held: &[Hold]) -> Result<Left, Box<Stop>> {
    let stop = |why: String, details: Value| Box::new(Stop { reason: "spend_limit", notice: format!("Stopped: {why}"), why, details });
    let h = held_for(held, &owner.scope());
    let person = match owner {
        Owner::Person(p) => p.as_str(),
        _ => "",
    };
    let m = super::month::key(now);
    let (all, _) = super::month::spent_by(s, &m, person);
    if let Some(limit) = s.spend.server_usd.filter(|l| all >= *l) {
        let e = super::enforce::SpendLimited { which: "server", month: m.clone(), spent_usd: all, limit_usd: limit, can_ask: false, pending: false };
        return Err(stop(e.to_string(), super::enforce::details(&e)));
    }
    if !person.is_empty() {
        if let Err(e) = super::enforce::left_now(s, person, now) {
            return Err(stop(e.to_string(), super::enforce::details(&e)));
        }
    }
    let ws = match owner {
        Owner::Workspace(w) => Some(w.as_str()),
        _ => None,
    };
    if let Some(w) = ws.and_then(|w| s.team.workspace(w)) {
        if let Some(why) = crate::team::spend::refusal(w, &crate::team::spend::spent_this_month(s, &w.id, now), "") {
            return Err(stop(why, json!({ "which": "workspace", "workspace": w.id, "can_ask": false, "pending": false })));
        }
    }
    let left = month_left(s, person, now, h.person, h.server, Left::none());
    Ok(team_left(s, ws, None, now, h.workspace, 0.0, left))
}

impl Kernel {
    /// The run's allowance now, read from the state as it is, with what
    /// the calls in flight hold.
    pub(crate) fn allowance(&self, run: &str, spender: Spender) -> Result<Left, Box<Stop>> {
        let held = self.spend_holds.now();
        self.read(|s| allowance_held(s, run, spender, Utc::now(), &held))
    }
}

#[cfg(test)]
#[path = "allowance_tests.rs"]
mod tests;
