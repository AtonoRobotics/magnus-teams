//! One answer to "may this work proceed now": the server's switch, the
//! switches below it, a pause and a person turned off at the
//! organisation's sign-in, folded into one `Hold` that carries its own
//! words (decisions/2026-11-13-one-hold.md). Each caller names whose work
//! it is (`Subject`) and how far along it is (`Phase`); the wider reading
//! of a switch is always the one taken, since stopping is always safe
//! (decisions/2026-11-11-kill-switch-levels.md).
use super::{agent_of, agent_of_person, connector_of, Halted, Scope, SERVER_REASON, TEAM_AGENT};
use crate::kernel::KillSwitchOn;
use crate::state::{Run, State};

/// Whose work it is: the person it is done for, the agent doing it, and
/// the connector it is about to reach, when it reaches one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subject {
    pub person: String,
    pub agent: String,
    pub connector: Option<String>,
}

impl Subject {
    /// A run's work, about to call `tool` when it names one.
    pub fn of_run(s: &State, run: &Run, tool: Option<&str>) -> Subject {
        Subject { person: run.person.clone(), agent: agent_of(s, run), connector: tool.and_then(connector_of).map(String::from) }
    }

    /// New work for `person`, by the team agent when `team`.
    pub fn new_work(person: &str, team: bool) -> Subject {
        let agent = if team { TEAM_AGENT.to_string() } else { agent_of_person(person) };
        Subject { person: person.into(), agent, connector: None }
    }

    /// The person's own agent reaching `app` for them outside any run: a
    /// watched read's poll.
    pub fn person_app(person: &str, app: Option<&str>) -> Subject {
        Subject { person: person.into(), agent: agent_of_person(person), connector: app.map(String::from) }
    }
}

/// How far along the work is, which decides what holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing has started yet: every switch, a pause, and a person turned
    /// off hold it.
    Start,
    /// A running run going on, or one of its tool calls: every switch in
    /// scope holds it. A pause does not (what runs goes on), nor does a
    /// person turned off (their apps refuse at `Call`; the run is not
    /// stopped).
    Continue,
    /// A call that goes out through the person's own sign-in to one of
    /// their apps: every switch in scope and a person turned off.
    Call,
}

/// What holds work, with its own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hold {
    /// The server's kill switch.
    Server,
    /// A switch below the server's.
    Switch(Scope),
    /// The agent is paused: nothing new starts for it.
    Paused { agent: String, person: String },
    /// The person is turned off at the organisation's sign-in.
    TurnedOff(String),
}

impl Hold {
    /// The reason a run this stops waits with.
    pub fn reason(&self) -> String {
        match self {
            Hold::Server => SERVER_REASON.into(),
            Hold::Switch(x) => x.reason(),
            Hold::Paused { agent, .. } => format!("paused:{agent}"),
            Hold::TurnedOff(_) => "turned_off".into(),
        }
    }

    /// What holds it, as a clause: "the kill switch is engaged".
    pub fn said(&self) -> String {
        match self {
            Hold::Server => "the kill switch is engaged".into(),
            Hold::Switch(x) => format!("{} is on", x.said()),
            Hold::Paused { agent, person } => format!("{} is paused", paused_who(agent, person)),
            Hold::TurnedOff(p) => format!("{p}'s account is turned off at the organisation's sign-in"),
        }
    }

    /// New work refused, as said to whoever asked for it.
    pub fn refusal(&self) -> String {
        match self {
            Hold::Server => KillSwitchOn.to_string(),
            Hold::Switch(x) => format!("{} is on: nothing new starts until an admin releases it.", capital(&x.said())),
            Hold::Paused { agent, .. } if agent == TEAM_AGENT => "The team agent is paused: nothing new starts until an admin resumes it.".into(),
            Hold::Paused { person, .. } => format!("{person}'s agent is paused: nothing new starts until an admin resumes it."),
            Hold::TurnedOff(p) => format!("{p}'s account is turned off at the organisation's sign-in: nothing new starts for them until they are back."),
        }
    }

    /// New work refused, as an error the API answers with a conflict.
    pub fn error(&self) -> anyhow::Error {
        match self {
            Hold::Server => KillSwitchOn.into(),
            _ => Halted(self.refusal()).into(),
        }
    }
}

fn paused_who(agent: &str, person: &str) -> String {
    if agent == TEAM_AGENT {
        "the team agent".into()
    } else {
        format!("{person}'s agent")
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// What holds `who`'s work at `phase`, if anything does: the server's
/// switch first, then the person's, the agent's and the connector's, then
/// a pause, then the person turned off.
pub fn may_proceed(s: &State, who: &Subject, phase: Phase) -> Option<Hold> {
    if s.kill_switch {
        return Some(Hold::Server);
    }
    let scopes = [Some(Scope::Person(who.person.clone())), Some(Scope::Agent(who.agent.clone())), who.connector.clone().map(Scope::Connector)];
    if let Some(x) = scopes.into_iter().flatten().find(|x| s.halts.on.contains_key(x)) {
        return Some(Hold::Switch(x));
    }
    if phase == Phase::Start && s.halts.paused.contains_key(&who.agent) {
        return Some(Hold::Paused { agent: who.agent.clone(), person: who.person.clone() });
    }
    if phase != Phase::Continue && s.turned_off.contains(&who.person) {
        return Some(Hold::TurnedOff(who.person.clone()));
    }
    None
}
