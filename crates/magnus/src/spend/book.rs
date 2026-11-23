//! The money ledger: what every paid model call cost, by month, person,
//! agent, model, project and team channel (decisions/2026-11-23-one-ledger-for-money.md).
//!
//! A line is written when the cost is folded, from the usage events, and
//! nothing that deletes content touches it: trashing a task, removing it
//! for good, or erasing a person takes their words, not their money. So a
//! person at their monthly limit cannot spend again by trashing what they
//! did, and a replay folds the same events to the same totals. The month's
//! totals (`month`), the team's (`team::spend`) and every limit read here.
//!
//! An erased person's lines stay under [`ERASED`] with no project, which
//! may be theirs: the server's and the team's totals still count them, and
//! nothing in the ledger names the person (`forget_tests.rs`).

use super::Change;
use crate::events::Ev;
use crate::log::Event;
use crate::state::State;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// Whose lines an erasure kept. Not a person id: an id holds no brackets
/// (`invitations::derive_id`).
pub const ERASED: &str = "(erased)";

/// The agent of a call the server made for itself, for no run and no one
/// person (a tool definition's screen).
pub const SERVER_AGENT: &str = "server";

/// One line of the ledger. Empty strings mean none: no person (the server's
/// own call, a workspace's), no project, no team channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line {
    /// `YYYY-MM`, the month the cost was folded in (UTC).
    pub month: String,
    pub person: String,
    /// `personal`, `team:<workspace>` or [`SERVER_AGENT`].
    pub agent: String,
    /// `adapter/model`.
    pub model: String,
    pub project: String,
    /// The workspace and channel whose team limits the cost counts against.
    pub workspace: String,
    pub channel: String,
}

/// Dollars by line.
#[derive(Clone, Debug, Default)]
pub struct Book {
    lines: BTreeMap<Line, f64>,
}

impl Book {
    /// A cost written to its line. Nothing, or nothing a price can be, is
    /// not written.
    pub fn add(&mut self, line: Line, usd: f64) {
        if usd.is_finite() && usd > 0.0 {
            *self.lines.entry(line).or_default() += usd;
        }
    }

    /// The month's lines.
    pub fn month<'a>(&'a self, month: &'a str) -> impl Iterator<Item = (&'a Line, f64)> + 'a {
        self.lines.iter().filter(move |(l, _)| l.month == month).map(|(l, usd)| (l, *usd))
    }

    /// The person's lines move under [`ERASED`], without a project; their
    /// dollars stay in every total.
    pub fn forget(&mut self, person: &str) {
        let theirs: Vec<Line> = self.lines.keys().filter(|l| l.person == person).cloned().collect();
        for line in theirs {
            let usd = self.lines.remove(&line).unwrap_or_default();
            let kept = Line { person: ERASED.into(), project: String::new(), ..line };
            *self.lines.entry(kept).or_default() += usd;
        }
    }
}

/// The model a usage event names, as `adapter/model`.
fn model_of(ev: &Ev) -> Option<&str> {
    match ev {
        Ev::VerifierReply { model, .. }
        | Ev::ReviewerReply { model, .. }
        | Ev::MemoryPass { model, .. }
        | Ev::SpokenWritten { model, .. }
        | Ev::KnowledgeExtraction { model, .. } => Some(model.as_str()),
        Ev::Spend(Change::Charged { model, .. }) => Some(model.as_str()),
        _ => None,
    }
}

/// The project a run's work belongs to: its top task's, else its team
/// channel's.
fn project_of(s: &State, run: &str) -> String {
    let Some(task) = s.authority(run) else { return String::new() };
    if let Some(p) = &task.project {
        return p.id.clone();
    }
    s.team.run_of(&task.task_id).and_then(|t| s.team.workspace(&t.workspace)?.channels.get(&t.channel)?.project.clone()).unwrap_or_default()
}

/// The line a run's cost goes on at `at`: its person, its agent (a
/// sub-agent's is its parent's), the model, its project and, for a team
/// run, its workspace and channel.
pub fn line_for_run(s: &State, run: &str, model: Option<&str>, at: DateTime<Utc>) -> Line {
    let r = s.runs.get(run);
    let model = match (model, r) {
        (Some(m), _) => m.to_string(),
        (None, Some(r)) => format!("{}/{}", r.model().0, r.model().1),
        (None, None) => String::new(),
    };
    let team = s.authority(run).and_then(|t| s.team.run_of(&t.task_id));
    Line {
        month: super::month::key(at),
        person: r.map(|r| r.person.clone()).unwrap_or_default(),
        agent: super::month::agent_of(s, run),
        model,
        project: project_of(s, run),
        workspace: team.map(|t| t.workspace.clone()).unwrap_or_default(),
        channel: team.map(|t| t.channel.clone()).unwrap_or_default(),
    }
}

/// The line of a call made for no run: a person's, a workspace's
/// (`workspace:<id>`) or the server's own (`server`).
pub fn line_for_owner(owner: &str, model: &str, at: DateTime<Utc>) -> Line {
    let (person, agent, workspace) = match owner.split_once(':') {
        Some(("person", p)) => (p.to_string(), "personal".to_string(), String::new()),
        Some(("workspace", w)) => (String::new(), format!("team:{w}"), w.to_string()),
        _ => (String::new(), SERVER_AGENT.to_string(), String::new()),
    };
    Line { month: super::month::key(at), person, agent, model: model.into(), project: String::new(), workspace, channel: String::new() }
}

impl State {
    /// A run's model cost, on the ledger: called with every usage event
    /// the run's own record folds.
    pub(crate) fn book_run_cost(&mut self, e: &Event, run: &str, usd: f64) {
        let line = line_for_run(self, run, model_of(&e.ev), e.at);
        self.spend.book.add(line, usd);
    }

    /// A `spend` change. A charge outside the usage events a run's own
    /// record folds: a side call's, failed after the provider charged for
    /// it, or one made for no run.
    pub(crate) fn fold_spend(&mut self, e: &Event, c: &Change) {
        let Change::Charged { run, owner, usage, .. } = c else {
            self.spend.apply(e.at, c);
            return;
        };
        match (run, owner) {
            (Some(run), _) => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.model_calls += 1;
                    r.aux_calls += 1;
                    r.usd += usage.cost_usd;
                    r.tokens.add(usage);
                }
                self.charge(run, e.at, usage.cost_usd);
                self.book_run_cost(e, run, usage.cost_usd);
            }
            (None, owner) => {
                let line = line_for_owner(owner.as_deref().unwrap_or(SERVER_AGENT), model_of(&e.ev).unwrap_or_default(), e.at);
                self.spend.book.add(line, usage.cost_usd);
            }
        }
    }
}

#[cfg(test)]
#[path = "book_tests.rs"]
mod tests;
