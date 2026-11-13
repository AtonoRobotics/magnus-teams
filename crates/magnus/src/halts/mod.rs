//! The kill switch below the whole server (spec rev 141, Admin: Kill
//! switch and Agents): one person's agents, one agent, or one connector,
//! beside the server's own (`Kernel::kill_switch`). Each stops new work and
//! running work in its scope at once; releasing it stops nothing more and
//! resumes nothing by itself, as the server's does (K53). An agent can also
//! be paused: nothing new starts for it, and what runs goes on.
//!
//! Agents are the team agent (`TEAM_AGENT`) and each person's own
//! (`person:<id>`). A run is the team agent's when it serves a team thread
//! (its sub-agents do too), its person's otherwise. A person's switch
//! covers everything done for them: their own agent and the team agent's
//! work they asked for (`decisions/2026-11-11-kill-switch-levels.md`).
mod agents;
mod hold;
mod kernel;
#[cfg(test)]
mod tests;

pub use hold::{may_proceed, Hold, Phase, Subject};
pub use kernel::{is_refusal, refused, Halted};

use crate::state::{Run, Schedule, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The team agent's id among the agents.
pub const TEAM_AGENT: &str = "team";
/// Why a run the server's switch stopped waits: what it has always been.
pub const SERVER_REASON: &str = "kill_switch";

/// What one switch below the server's covers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "level", content = "id", rename_all = "snake_case")]
pub enum Scope {
    /// Everything done for one person.
    Person(String),
    /// One agent: `TEAM_AGENT` or `person:<id>`.
    Agent(String),
    /// One connector (an app), for every agent.
    Connector(String),
}

impl Scope {
    /// From the level and target an admin names.
    pub fn parse(level: &str, id: &str) -> Option<Scope> {
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        match level {
            "person" => Some(Scope::Person(id.into())),
            "agent" if id == TEAM_AGENT || id.strip_prefix("person:").is_some_and(|p| !p.is_empty()) => Some(Scope::Agent(id.into())),
            "connector" => Some(Scope::Connector(id.into())),
            _ => None,
        }
    }

    pub fn level(&self) -> &'static str {
        match self {
            Scope::Person(_) => "person",
            Scope::Agent(_) => "agent",
            Scope::Connector(_) => "connector",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Scope::Person(id) | Scope::Agent(id) | Scope::Connector(id) => id,
        }
    }

    /// Why a run it stopped waits: the server's reason, with its scope.
    pub fn reason(&self) -> String {
        format!("{SERVER_REASON}:{}:{}", self.level(), self.id())
    }

    /// The switch's words, for a refusal or a notice.
    pub fn said(&self) -> String {
        match self {
            Scope::Person(p) => format!("the kill switch on {p}'s agents"),
            Scope::Agent(a) if a == TEAM_AGENT => "the kill switch on the team agent".into(),
            Scope::Agent(a) => format!("the kill switch on {}'s agent", a.trim_start_matches("person:")),
            Scope::Connector(c) => format!("the kill switch on {c}"),
        }
    }
}

/// One change to the switches below the server's, as the log keeps it. A
/// retired form keeps its reader here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// Pulled. `name` is who pulled it, as people see them.
    Engaged {
        scope: Scope,
        #[serde(default)]
        name: String,
    },
    Released {
        scope: Scope,
    },
    /// Nothing new starts for the agent; what runs goes on.
    Paused {
        agent: String,
        #[serde(default)]
        name: String,
    },
    Unpaused {
        agent: String,
    },
}

/// Who pulled a switch or paused an agent, and when.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Pulled {
    /// As the log names them (`person:<id>`, `admin:<sub>`).
    pub by: String,
    pub name: String,
    pub at: DateTime<Utc>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    pub on: BTreeMap<Scope, Pulled>,
    pub paused: BTreeMap<String, Pulled>,
}

impl Ledger {
    pub fn apply(&mut self, c: &Change, actor: &str, at: DateTime<Utc>) {
        let pulled = |name: &str| Pulled { by: actor.into(), name: if name.is_empty() { actor.into() } else { name.into() }, at };
        match c {
            Change::Engaged { scope, name } => {
                self.on.insert(scope.clone(), pulled(name));
            }
            Change::Released { scope } => {
                self.on.remove(scope);
            }
            Change::Paused { agent, name } => {
                self.paused.insert(agent.clone(), pulled(name));
            }
            Change::Unpaused { agent } => {
                self.paused.remove(agent);
            }
        }
    }
}

/// Whether a run waits on a kill switch, the server's or one below it.
pub fn is_kill(reason: &str) -> bool {
    reason == SERVER_REASON || reason.strip_prefix(SERVER_REASON).is_some_and(|r| r.starts_with(':'))
}

/// The agent a person's own work is done by.
pub fn agent_of_person(person: &str) -> String {
    format!("person:{person}")
}

/// The agent a run is: the team agent's when it serves a team thread.
pub fn agent_of(s: &State, run: &Run) -> String {
    if s.team.run_of(&run.task).is_some() {
        TEAM_AGENT.into()
    } else {
        agent_of_person(&run.person)
    }
}

/// The app a tool belongs to: a connector's `<app>__<tool>`, or the
/// appliance's own tool's app.
pub fn connector_of(tool: &str) -> Option<&str> {
    match tool.split_once("__") {
        Some((app, _)) => Some(app),
        None => crate::tools::builtin(tool).map(|b| b.app),
    }
}

/// Why a run may not go on now, if it may not: the switch that holds it and
/// the reason it waits with. `tool`, when it is about to call one, brings
/// in that tool's connector's switch.
pub fn halt(s: &State, run: &Run, tool: Option<&str>) -> Option<(String, String)> {
    may_proceed(s, &Subject::of_run(s, run, tool), Phase::Continue).map(|h| (h.reason(), h.said()))
}

/// Why new work for `person` may not start, if it may not: `team` when the
/// team agent would do it. The server's switch, the switches below it, a
/// pause, and the person turned off.
pub fn holds_new(s: &State, person: &str, team: bool) -> Option<String> {
    may_proceed(s, &Subject::new_work(person, team), Phase::Start).map(|h| h.refusal())
}

/// Whether a running run falls under a switch now being pulled: for a
/// connector, a run that has called it (the work that needs it).
pub fn covers(s: &State, scope: &Scope, run: &Run) -> bool {
    match scope {
        Scope::Person(p) => run.person == *p,
        Scope::Agent(a) => agent_of(s, run) == *a,
        Scope::Connector(c) => run.intents.values().any(|i| connector_of(&i.tool) == Some(c.as_str())),
    }
}

/// The schedules due now that may fire: none under the server's switch,
/// and none for a person whose agent is held.
pub fn due_schedules(s: &State, now: DateTime<Utc>) -> Vec<Schedule> {
    if s.kill_switch {
        return vec![];
    }
    s.schedules.values().filter(|x| x.next_due().is_some_and(|t| t <= now) && holds_new(s, &x.person, false).is_none()).cloned().collect()
}

/// The tools a run is offered, without those of a connector switched off.
pub fn offered(s: &State, specs: Vec<magnus_adapter_proto::ToolSpec>) -> Vec<magnus_adapter_proto::ToolSpec> {
    specs.into_iter().filter(|t| connector_of(&t.name).is_none_or(|c| !s.halts.on.contains_key(&Scope::Connector(c.into())))).collect()
}
