//! What the log says of outside agents: each one connected, who it is
//! admitted for, the tasks Magnus sent it, and the requests it sent Magnus.
//! Folded from `Ev::OutsideAgent`; a retired change keeps its reader here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// How Magnus authenticates to an outside agent, as its card's security
/// schemes chose: what the proxy attaches from the vault on each call.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scheme {
    /// The card declares no security: nothing is attached.
    None,
    /// An API key in the header the card names.
    ApiKey { header: String },
    /// An HTTP bearer token.
    Bearer,
    /// OAuth 2's client-credentials flow at the card's token URL.
    OAuth2 { token_url: String, scopes: Vec<String> },
    /// A client certificate (mutual TLS).
    MutualTls,
}

impl Scheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scheme::None => "none",
            Scheme::ApiKey { .. } => "api_key",
            Scheme::Bearer => "bearer",
            Scheme::OAuth2 { .. } => "oauth2",
            Scheme::MutualTls => "mutual_tls",
        }
    }
}

/// Who an outside agent asks: one person, or a shared project (a workspace).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Person(String),
    Project(String),
}

impl Target {
    /// The person's or project's id.
    pub fn id(&self) -> &str {
        match self {
            Target::Person(id) | Target::Project(id) => id,
        }
    }

    /// Whether its id is one an id can be: 1 to `MAX_TARGET_ID_CHARS`
    /// characters, none a control character, a quote or a backslash, so
    /// it is never a Cedar entity that cannot be named.
    pub fn valid(&self) -> bool {
        let id = self.id();
        !id.is_empty() && id.chars().count() <= super::MAX_TARGET_ID_CHARS && !id.chars().any(|c| c.is_control() || c == '"' || c == '\\')
    }
}

/// One outside agent a member connected, by its agent card.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutsideAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub card_url: String,
    /// The JSON-RPC endpoint its card declared: every task goes here, and nowhere else.
    pub endpoint: String,
    pub scheme: Scheme,
    /// The member who connected it: its tasks are sent from their runs alone.
    pub person: String,
    #[serde(default)]
    pub skills: Vec<String>,
    /// The key id the card's verified signature names.
    #[serde(default)]
    pub key_id: String,
}

/// One change to the outside agents.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Connected {
        agent: OutsideAgent,
    },
    Disconnected {
        agent: String,
    },
    /// A Cedar policy admits the agent for one person or one shared project
    /// (in a project, it is a guest there).
    Admitted {
        agent: String,
        to: Target,
        policy: String,
    },
    /// A task Magnus is about to send, recorded before the send (invariant 3).
    TaskOpened {
        agent: String,
        run: String,
        call_id: String,
        key: String,
        message_id: String,
    },
    /// The outside agent's id for the task, recorded as soon as it answers.
    TaskStarted {
        key: String,
        task: String,
        state: String,
    },
    /// A sent task's state, as GetTask or CancelTask last read it.
    TaskMoved {
        key: String,
        state: String,
    },
    /// A request an outside agent sent, landed with guest standing.
    RequestReceived {
        agent: String,
        task: String,
        message_id: String,
        to: Target,
        #[serde(default)]
        text: String,
        #[serde(default)]
        item: Option<String>,
        /// The project message it was posted as.
        #[serde(default)]
        message: Option<(String, String)>,
    },
    /// A person took the request up: it runs as their own run.
    RequestTakenUp {
        task: String,
        run: String,
        person: String,
    },
    /// The reply waits in the person's Airlock, with why.
    ReplyHeld {
        task: String,
        item: String,
        #[serde(default)]
        draft: String,
        #[serde(default)]
        held_because: String,
    },
    /// The reply went to the outside agent (it reads it with GetTask).
    Replied {
        task: String,
        #[serde(default)]
        reply: String,
        by: String,
    },
    RequestEnded {
        task: String,
        state: String,
        #[serde(default)]
        reason: String,
    },
    /// The person's standing release: replies to this agent for this exact
    /// request leave without waiting in their Airlock, past the checks.
    /// `key` is a digest of the request's words, never the words.
    StandingReleaseSet {
        person: String,
        agent: String,
        key: String,
        active: bool,
    },
}

/// A task Magnus sent, by the action's idempotency key.
#[derive(Clone, Debug, PartialEq)]
pub struct Outbound {
    pub agent: String,
    pub run: String,
    pub call_id: String,
    pub message_id: String,
    pub task: Option<String>,
    pub state: String,
}

/// A request an outside agent sent, by Magnus's task id for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Inbound {
    pub agent: String,
    pub task: String,
    pub message_id: String,
    pub to: Target,
    pub text: String,
    pub item: Option<String>,
    pub message: Option<(String, String)>,
    pub at: DateTime<Utc>,
    pub run: Option<String>,
    pub person: Option<String>,
    pub state: String,
    pub reply: Option<String>,
    pub held: Option<String>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    agents: BTreeMap<String, OutsideAgent>,
    admissions: BTreeMap<String, Vec<(Target, String)>>,
    outbound: BTreeMap<String, Outbound>,
    inbound: BTreeMap<String, Inbound>,
    standing: BTreeSet<(String, String, String)>,
}

impl Ledger {
    pub fn apply(&mut self, at: DateTime<Utc>, c: &Change) {
        match c {
            Change::Connected { agent } => {
                self.agents.insert(agent.id.clone(), agent.clone());
            }
            Change::Disconnected { agent } => {
                self.agents.remove(agent);
                self.admissions.remove(agent);
                self.standing.retain(|(_, a, _)| a != agent);
            }
            Change::Admitted { agent, to, policy } => {
                let list = self.admissions.entry(agent.clone()).or_default();
                list.retain(|(t, _)| t != to);
                list.push((to.clone(), policy.clone()));
            }
            Change::TaskOpened { agent, run, call_id, key, message_id } => {
                let o = Outbound { agent: agent.clone(), run: run.clone(), call_id: call_id.clone(), message_id: message_id.clone(), task: None, state: super::SENDING.into() };
                self.outbound.entry(key.clone()).or_insert(o);
            }
            Change::TaskStarted { key, task, state } => {
                if let Some(o) = self.outbound.get_mut(key) {
                    o.task = Some(task.clone());
                    o.state = state.clone();
                }
            }
            Change::TaskMoved { key, state } => {
                if let Some(o) = self.outbound.get_mut(key) {
                    o.state = state.clone();
                }
            }
            Change::RequestReceived { agent, task, message_id, to, text, item, message } => {
                let i = Inbound {
                    agent: agent.clone(),
                    task: task.clone(),
                    message_id: message_id.clone(),
                    to: to.clone(),
                    text: text.clone(),
                    item: item.clone(),
                    message: message.clone(),
                    at,
                    run: None,
                    person: None,
                    state: super::SUBMITTED.into(),
                    reply: None,
                    held: None,
                };
                self.inbound.entry(task.clone()).or_insert(i);
            }
            Change::RequestTakenUp { task, run, person } => {
                if let Some(i) = self.inbound.get_mut(task) {
                    i.run = Some(run.clone());
                    i.person = Some(person.clone());
                    i.state = super::WORKING.into();
                }
            }
            Change::ReplyHeld { task, item, .. } => {
                if let Some(i) = self.inbound.get_mut(task) {
                    i.held = Some(item.clone());
                }
            }
            Change::Replied { task, reply, .. } => {
                if let Some(i) = self.inbound.get_mut(task) {
                    i.reply = Some(reply.clone());
                    i.state = super::COMPLETED.into();
                }
            }
            Change::RequestEnded { task, state, .. } => {
                if let Some(i) = self.inbound.get_mut(task) {
                    i.state = state.clone();
                }
            }
            Change::StandingReleaseSet { person, agent, key, active } => {
                let k = (person.clone(), agent.clone(), key.clone());
                if *active {
                    self.standing.insert(k);
                } else {
                    self.standing.remove(&k);
                }
            }
        }
    }

    pub fn agent(&self, id: &str) -> Option<&OutsideAgent> {
        self.agents.get(id)
    }

    /// The agents a member connected.
    pub fn of(&self, person: &str) -> Vec<&OutsideAgent> {
        self.agents.values().filter(|a| a.person == person).collect()
    }

    /// The agent a member names, by id or by name.
    pub fn named(&self, person: &str, name: &str) -> Option<&OutsideAgent> {
        let want = name.trim().to_lowercase();
        self.of(person).into_iter().find(|a| a.id == name || a.name.to_lowercase() == want)
    }

    /// Every Cedar policy admitting `agent`, and what each admits it for.
    pub fn admissions(&self, agent: &str) -> &[(Target, String)] {
        self.admissions.get(agent).map_or(&[], Vec::as_slice)
    }

    /// Whether any agent is admitted for a person, and for a project: the
    /// skills the published card lists follow from these.
    pub fn admitted_kinds(&self) -> (bool, bool) {
        let all = || self.admissions.iter().filter(|(a, _)| self.agents.contains_key(*a)).flat_map(|(_, l)| l.iter());
        (all().any(|(t, _)| matches!(t, Target::Person(_))), all().any(|(t, _)| matches!(t, Target::Project(_))))
    }

    pub fn outbound(&self, key: &str) -> Option<&Outbound> {
        self.outbound.get(key)
    }

    /// The task sent `agent` that it calls `task`, with its key.
    pub fn sent_as(&self, agent: &str, task: &str) -> Option<(String, Outbound)> {
        self.outbound.iter().find(|(_, o)| o.agent == agent && o.task.as_deref() == Some(task)).map(|(k, o)| (k.clone(), o.clone()))
    }

    /// The tasks sent to an outside agent that it has not finished.
    pub fn open_outbound(&self, pred: impl Fn(&Outbound) -> bool) -> Vec<(String, Outbound)> {
        self.outbound.iter().filter(|(_, o)| o.task.is_some() && !super::terminal(&o.state) && pred(o)).map(|(k, o)| (k.clone(), o.clone())).collect()
    }

    /// The request `agent` sent as `task`, if it sent it: a task id is the
    /// sending principal's own.
    pub fn inbound_of(&self, agent: &str, task: &str) -> Option<&Inbound> {
        self.inbound.get(task).filter(|i| i.agent == agent)
    }

    pub fn inbound(&self, task: &str) -> Option<&Inbound> {
        self.inbound.get(task)
    }

    /// The request a run was taken up for.
    pub fn served_by(&self, run: &str) -> Option<&Inbound> {
        self.inbound.values().find(|i| i.run.as_deref() == Some(run))
    }

    /// The requests from `agent` that are not finished.
    pub fn open_inbound(&self, agent: &str) -> Vec<Inbound> {
        self.inbound.values().filter(|i| i.agent == agent && !super::terminal(&i.state)).cloned().collect()
    }

    /// How many requests `agent` sent since `since`.
    pub fn received_since(&self, agent: &str, since: DateTime<Utc>) -> usize {
        self.inbound.values().filter(|i| i.agent == agent && i.at >= since).count()
    }

    pub fn has_standing_release(&self, person: &str, agent: &str, key: &str) -> bool {
        self.standing.contains(&(person.to_string(), agent.to_string(), key.to_string()))
    }
}

/// An erased person's agents, their admissions, the tasks their runs sent
/// and the requests to them or taken up by them leave with them.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let theirs: BTreeSet<String> = self.agents.values().filter(|a| who.is(&a.person)).map(|a| a.id.clone()).collect();
        self.agents.retain(|id, _| !theirs.contains(id));
        self.admissions.retain(|id, _| !theirs.contains(id));
        for list in self.admissions.values_mut() {
            list.retain(|(t, _)| !matches!(t, Target::Person(p) if who.is(p)));
        }
        self.outbound.retain(|_, o| !theirs.contains(&o.agent) && !who.runs.contains(&o.run));
        self.inbound.retain(|_, i| !theirs.contains(&i.agent) && !matches!(&i.to, Target::Person(p) if who.is(p)) && !i.person.as_deref().is_some_and(|p| who.is(p)));
        self.standing.retain(|(p, a, _)| !who.is(p) && !theirs.contains(a));
    }
}
