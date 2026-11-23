//! A person's arrangements with Magnus: schedules (with the zone their
//! daily times are read in), grants of standing authority (#235),
//! commitments (#236), and their answers to cards proposed in chat.

use crate::events::Ev;
use crate::log::Event;
use crate::tools::Label;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// A schedule's record. What its runs are asked (its text), its title and
/// scope are recalled from the event `seq` names, as a task's are.
#[derive(Clone, Debug)]
pub struct Schedule {
    pub id: String,
    pub person: String,
    pub every_minutes: Option<u32>,
    pub daily_at: Option<String>,
    pub created: DateTime<Utc>,
    pub last_fired: Option<DateTime<Utc>>,
    pub budget: Option<crate::config::Budget>,
    pub card: Option<String>,
    pub label: Label,
    /// The event that set it: where its words are recalled from.
    pub seq: i64,
    /// Firings that could not start a task: when, and why.
    pub failures: Vec<(DateTime<Utc>, String)>,
    pub until: Option<DateTime<Utc>>,
    pub max_runs: Option<u32>,
    /// Runs it has started.
    pub started: u32,
    /// The latest firing that started nothing, because the last run was
    /// still open: when, which task, why.
    pub skipped: Option<(DateTime<Utc>, String, String)>,
    /// Every reason it is paused now, each with what was said of it: a
    /// pause adds its cause, a release lifts only its own, and it starts
    /// no runs while any is left.
    pub pauses: BTreeMap<PauseCause, String>,
    /// Why it stopped starting runs: the strongest cause's words (an
    /// admin's, then its person's, then a waiting run's). Kept from `pauses`.
    pub paused: Option<String>,
    /// The task whose run it is held on, waiting on a person. Kept from `pauses`.
    pub paused_on: Option<String>,
    /// An admin suspended it: only an admin lifts it. Kept from `pauses`.
    pub paused_by_admin: bool,
    /// From when failures count toward pausing it again.
    pub resumed: Option<DateTime<Utc>>,
    /// Why it is over, once it is: an ended schedule stays, marked.
    pub ended: Option<String>,
    /// The conversation it was set up in, if any: where its runs post (`schedule_thread::of`).
    pub thread: Option<String>,
    /// The zone its daily time is read in: its person's, kept current as
    /// they change it, else the server's (`cadence::zone_of`).
    pub zone: crate::cadence::Zone,
}

/// Why a schedule is paused (decisions/2026-11-23-a-schedule-knows-why-it-is-paused.md),
/// read from the existing events' fields: a `SchedulePaused` marked `admin`
/// is an admin's suspension, one with a task a hold on that run, any other
/// a pause for its person. Ordered weakest first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PauseCause {
    /// Its run (this task) waits on its person: lifted by the kernel once
    /// that run no longer waits.
    Held(String),
    /// For its person: they (or someone over them) paused it, its runs
    /// failed in a row, or they were turned off. Lifted when they resume it.
    Person,
    /// An admin suspended it: lifted by an admin.
    Admin,
}

impl Schedule {
    /// The derived fields, from `pauses`.
    fn settle_pauses(&mut self) {
        self.paused = self.pauses.values().next_back().cloned();
        self.paused_on = self.pauses.keys().find_map(|c| match c {
            PauseCause::Held(t) => Some(t.clone()),
            _ => None,
        });
        self.paused_by_admin = self.pauses.contains_key(&PauseCause::Admin);
    }

    /// Whether it is held on a waiting run.
    pub fn held(&self) -> bool {
        self.pauses.keys().any(|c| matches!(c, PauseCause::Held(_)))
    }

    /// Whether its person (or someone over them) paused it for them.
    pub fn paused_for_person(&self) -> bool {
        self.pauses.contains_key(&PauseCause::Person)
    }
}

/// A person's standing authority over a category of their own work
/// (#235). Its title is recalled from the event `seq` names (the latest
/// setting), as a schedule's words are.
#[derive(Clone, Debug)]
pub struct Grant {
    pub id: String,
    pub person: String,
    pub apps: Vec<String>,
    /// Exclusions the person lifted: `send`, `irreversible`.
    pub allow: Vec<String>,
    /// `day`, `week` or `month`.
    pub period: String,
    pub max_usd: f64,
    /// The policy the kernel wrote from these: what it covers.
    pub cedar: String,
    /// The card it was first set from.
    pub card: Option<String>,
    /// The taint of the words of its title, when not the person's own.
    pub label: Label,
    pub seq: i64,
    /// When it was set first, and by whom (as the log names them).
    pub at: DateTime<Utc>,
    pub by: String,
    /// When it was last edited.
    pub edited: Option<DateTime<Utc>>,
    pub revoked: Option<DateTime<Utc>>,
    /// The actions taken under it, oldest first.
    pub actions: Vec<GrantAction>,
    /// What its tasks' model calls cost, as each was charged: kept with the
    /// grant, so deleting a task gives none of it back (review of #278).
    pub charges: Vec<(DateTime<Utc>, f64)>,
}

/// An action a grant covered.
#[derive(Clone, Debug, Serialize)]
pub struct GrantAction {
    pub tool: String,
    pub task: String,
    pub run: String,
    pub call_id: String,
    pub at: DateTime<Utc>,
}

/// Something the person or Magnus said they would do (#236). Its words
/// (what, for whom, the quote, notes) are recalled from its events, by
/// `steps`; the rest is here. Overdue is never stored: it is open with
/// `by` past (`commitments::overdue`).
#[derive(Clone, Debug)]
pub struct Commitment {
    pub id: String,
    pub person: String,
    /// `person` or `magnus`.
    pub who: String,
    pub source: Value,
    /// The taint of the run that recorded it.
    pub label: Label,
    /// When it was recorded, and by whom (as the log names them).
    pub at: DateTime<Utc>,
    pub by_actor: String,
    /// When it last changed.
    pub changed: DateTime<Utc>,
    /// Its recording event.
    pub seq: i64,
    pub status: String,
    pub by: Option<String>,
    /// Each event of it, oldest first, with its status and date after it:
    /// what it was as a run that started at any point saw it.
    pub steps: Vec<CommitmentStep>,
}

#[derive(Clone, Debug)]
pub struct CommitmentStep {
    pub seq: i64,
    pub status: String,
    pub by: Option<String>,
}

impl Commitment {
    /// Its status and date as they stood before the event `seq`, and the
    /// events that made it so; None when it was recorded after.
    pub fn before(&self, seq: i64) -> Option<(&str, Option<&str>, Vec<i64>)> {
        let steps: Vec<&CommitmentStep> = self.steps.iter().filter(|s| s.seq < seq).collect();
        let last = steps.last()?;
        Some((last.status.as_str(), last.by.as_deref(), steps.iter().map(|s| s.seq).collect()))
    }
}

/// The person's answer to a card proposed in chat (a schedule or a
/// suggestion), when they gave it and whose card it was.
#[derive(Clone, Debug, PartialEq)]
pub struct CardState {
    pub answer: Answer,
    pub at: DateTime<Utc>,
    pub person: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Accepted: the schedule it made.
    Confirmed(String),
    /// Accepted: the task it started.
    Started(String),
    /// Accepted: the grant it set (#235).
    Granted(String),
    /// "Not now".
    Later,
    /// Declined, with their reason.
    Declined(Option<String>),
}

#[derive(Default, Debug)]
pub struct Arrangements {
    schedules: BTreeMap<String, Schedule>,
    /// Each person's grants (#235), revoked ones among them, by id.
    grants: BTreeMap<String, Grant>,
    /// Each person's commitment ledger (#236), by id.
    commitments: BTreeMap<String, Commitment>,
    /// Cards from chat (schedules and suggestions) the person answered, by
    /// card id: with the cards in their turns' results, the suggestion
    /// ledger (`suggestions::ledger`).
    cards: HashMap<String, CardState>,
    /// Optional quarantine extraction schemas keyed by schedule id (W1.1, N12).
    quarantine_schemas: HashMap<String, Value>,
}

/// Their schedules, their grants (none covers whoever is next known by
/// their name), their commitments, whose words went with theirs (#236),
/// and their answers to cards, a decline's reason among them.
impl crate::forget::Forget for Arrangements {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let p = who.person.as_str();
        self.schedules.retain(|_, x| x.person != p);
        self.quarantine_schemas.retain(|id, _| self.schedules.contains_key(id));
        self.grants.retain(|_, g| g.person != p);
        self.commitments.retain(|_, c| c.person != p);
        self.cards.retain(|_, c| c.person.as_deref() != Some(p));
    }
}

impl Arrangements {
    pub fn quarantine_schema(&self, schedule_id: &str) -> Option<&Value> {
        self.quarantine_schemas.get(schedule_id)
    }

    /// The schedule (but `ScheduleFired`), grant, commitment and card
    /// kinds; `prefs` is each person's settings, for a new schedule's zone.
    /// `State::apply` routes no other kind here.
    pub(crate) fn fold(&mut self, e: &Event, prefs: &HashMap<String, Value>) {
        match &e.ev {
            Ev::ScheduleSet { id, person, every_minutes, daily_at, budget, card, label, until, max_runs, thread, quarantine_schema, .. } => {
                if let Some(schema) = quarantine_schema {
                    self.quarantine_schemas.insert(id.clone(), schema.clone());
                } else {
                    self.quarantine_schemas.remove(id);
                }
                self.schedules.insert(
                    id.clone(),
                    Schedule {
                        id: id.clone(),
                        zone: crate::cadence::zone_of(prefs.get(person)),
                        person: person.clone(),
                        every_minutes: *every_minutes,
                        daily_at: daily_at.clone(),
                        created: e.at,
                        last_fired: None,
                        budget: budget.clone(),
                        card: card.clone(),
                        label: label.unwrap_or_default(),
                        seq: e.seq,
                        failures: vec![],
                        until: *until,
                        max_runs: *max_runs,
                        started: 0,
                        skipped: None,
                        pauses: BTreeMap::new(),
                        paused: None,
                        paused_on: None,
                        paused_by_admin: false,
                        resumed: None,
                        ended: None,
                        thread: thread.clone(),
                    },
                );
                if let Some(c) = card {
                    self.cards.insert(c.clone(), CardState { answer: Answer::Confirmed(id.clone()), at: e.at, person: Some(person.clone()) });
                }
            }
            Ev::GrantSet { id, person, apps, allow, period, max_usd, card, cedar, label, .. } => {
                let g = self.grants.entry(id.clone()).or_insert_with(|| Grant {
                    id: id.clone(),
                    person: person.clone(),
                    apps: vec![],
                    allow: vec![],
                    period: String::new(),
                    max_usd: 0.0,
                    cedar: String::new(),
                    card: card.clone(),
                    label: Label::PRINCIPAL,
                    seq: e.seq,
                    at: e.at,
                    by: e.actor.clone(),
                    edited: None,
                    revoked: None,
                    actions: vec![],
                    charges: vec![],
                });
                if g.seq != e.seq {
                    g.edited = Some(e.at);
                }
                g.apps = apps.clone();
                g.allow = allow.clone();
                g.period = period.clone();
                g.max_usd = *max_usd;
                g.cedar = cedar.clone();
                g.label = label.unwrap_or_default();
                g.seq = e.seq;
                // A card accepted sets it; a proposal handled with it is
                // answered by the task it started, which follows.
                if let Some(c) = card.as_ref().filter(|c| !self.cards.contains_key(*c)) {
                    self.cards.insert(c.clone(), CardState { answer: Answer::Granted(id.clone()), at: e.at, person: Some(person.clone()) });
                }
            }
            Ev::GrantRevoked { id, .. } => {
                if let Some(g) = self.grants.get_mut(id).filter(|g| g.revoked.is_none()) {
                    g.revoked = Some(e.at);
                }
            }
            Ev::GrantUsed { grant, task, run, call_id, tool } => {
                if let Some(g) = self.grants.get_mut(grant) {
                    g.actions.push(GrantAction { tool: tool.clone(), task: task.clone(), run: run.clone(), call_id: call_id.clone(), at: e.at });
                }
            }
            Ev::CommitmentRecorded { id, person, who, by, source, label, .. } => {
                let by = by.clone().filter(|b| !b.is_empty());
                self.commitments.entry(id.clone()).or_insert_with(|| Commitment {
                    id: id.clone(),
                    person: person.clone(),
                    who: who.clone(),
                    source: source.clone(),
                    label: *label,
                    at: e.at,
                    by_actor: e.actor.clone(),
                    changed: e.at,
                    seq: e.seq,
                    status: "open".into(),
                    by: by.clone(),
                    steps: vec![CommitmentStep { seq: e.seq, status: "open".into(), by }],
                });
            }
            Ev::CommitmentChanged { id, person, by, status, .. } => {
                if let Some(c) = self.commitments.get_mut(id).filter(|c| &c.person == person) {
                    if let Some(b) = by {
                        c.by = Some(b.clone()).filter(|b| !b.is_empty());
                    }
                    if let Some(s) = status {
                        c.status = s.clone();
                    }
                    c.changed = e.at;
                    c.steps.push(CommitmentStep { seq: e.seq, status: c.status.clone(), by: c.by.clone() });
                }
            }
            Ev::CommitmentRemoved { id, person } => {
                if self.commitments.get(id).is_some_and(|c| &c.person == person) {
                    self.commitments.remove(id);
                }
            }
            Ev::CardDeclined { card, person, reason } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Declined(reason.clone()), at: e.at, person: person.clone() });
            }
            Ev::CardDeferred { card, person } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Later, at: e.at, person: person.clone() });
            }
            Ev::CardAccepted { card, person, task } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Started(task.clone()), at: e.at, person: Some(person.clone()) });
            }
            Ev::ScheduleRemoved { id, .. } => {
                self.schedules.remove(id);
            }
            Ev::ScheduleEnded { id, reason, .. } => {
                // It stays, marked with why: its card says it ended, with
                // its latest run. Only a cancelled one is gone (#264). The
                // end is terminal: no pause outlives it.
                if let Some(s) = self.schedules.get_mut(id) {
                    s.ended = Some(reason.clone());
                    s.pauses.clear();
                    s.settle_pauses();
                }
            }
            Ev::ScheduleSkipped { id, task, reason } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.last_fired = Some(e.at);
                    s.skipped = Some((e.at, task.clone(), reason.clone()));
                }
            }
            Ev::SchedulePaused { id, reason, task, admin, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    let cause = match (task, admin) {
                        (_, true) => PauseCause::Admin,
                        (Some(t), false) => PauseCause::Held(t.clone()),
                        (None, false) => PauseCause::Person,
                    };
                    s.pauses.insert(cause, reason.clone());
                    s.settle_pauses();
                }
            }
            Ev::ScheduleChanged { id, every_minutes, daily_at, until, max_runs, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.every_minutes = *every_minutes;
                    s.daily_at = daily_at.clone();
                    s.until = *until;
                    s.max_runs = *max_runs;
                    // A new end puts an ended one back: edits only move the
                    // end forward, so it runs again (#264).
                    s.ended = None;
                }
            }
            Ev::ScheduleResumed { id, source, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    // Each release lifts only its own cause: the kernel's
                    // (no source) a hold on a waiting run, the console an
                    // admin's suspension, the schedule's person their own
                    // pause (and a suspension, which the app lets them lift
                    // only when they administer), anyone else a suspension.
                    let lifts: &[PauseCause] = if e.actor == "kernel" && source.is_none() {
                        &[]
                    } else if source.as_deref() == Some("console") || e.actor != format!("person:{}", s.person) {
                        &[PauseCause::Admin]
                    } else {
                        &[PauseCause::Person, PauseCause::Admin]
                    };
                    if lifts.is_empty() {
                        s.pauses.retain(|c, _| !matches!(c, PauseCause::Held(_)));
                    }
                    let lifted = lifts.iter().filter(|c| s.pauses.remove(*c).is_some()).count();
                    s.settle_pauses();
                    // Failures count afresh only after someone resumes a
                    // schedule paused for its person or by an admin.
                    if lifted > 0 {
                        s.resumed = Some(e.at);
                    }
                    // The next run is due from now, not from long ago.
                    s.last_fired = Some(e.at);
                }
            }
            // A firing that failed still counts as fired: the next one is due as usual.
            Ev::ScheduleFailed { id, reason } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.last_fired = Some(e.at);
                    s.failures.push((e.at, reason.clone()));
                }
            }
            _ => {}
        }
    }

    /// Daily times follow the person's zone (owner, 2026-11-14).
    pub(crate) fn set_zone(&mut self, person: &str, zone: crate::cadence::Zone) {
        self.schedules.values_mut().filter(|x| x.person == person).for_each(|x| x.zone = zone);
    }

    /// A schedule's run counts against it as it is created.
    pub(crate) fn started(&mut self, schedule: &str, at: DateTime<Utc>) {
        if let Some(s) = self.schedules.get_mut(schedule) {
            s.last_fired = Some(at);
            s.started += 1;
            s.skipped = None;
        }
    }

    /// A schedule fired; `counted` when the task's creation counted it.
    pub(crate) fn fired(&mut self, schedule: &str, counted: bool, at: DateTime<Utc>) {
        if let Some(s) = self.schedules.get_mut(schedule) {
            s.last_fired = Some(at);
            if !counted {
                s.started += 1;
            }
            s.skipped = None;
        }
    }

    /// A model call's cost, kept with the grant its run acts under.
    pub(crate) fn charge_grant(&mut self, grant: &str, at: DateTime<Utc>, usd: f64) {
        if let Some(g) = self.grants.get_mut(grant) {
            g.charges.push((at, usd));
        }
    }

    pub fn schedules(&self) -> &BTreeMap<String, Schedule> {
        &self.schedules
    }

    /// The person's schedules, ended ones among them.
    pub fn schedules_of<'a>(&'a self, person: &'a str) -> impl Iterator<Item = &'a Schedule> + 'a {
        self.schedules.values().filter(move |x| x.person == person)
    }

    pub fn grants(&self) -> &BTreeMap<String, Grant> {
        &self.grants
    }

    pub fn commitments(&self) -> &BTreeMap<String, Commitment> {
        &self.commitments
    }

    pub fn cards(&self) -> &HashMap<String, CardState> {
        &self.cards
    }

    #[cfg(test)]
    pub(crate) fn grants_mut(&mut self) -> &mut BTreeMap<String, Grant> {
        &mut self.grants
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("schedules", format!("{:?}", self.schedules)),
            ("grants", format!("{:?}", self.grants)),
            ("commitments", format!("{:?}", self.commitments)),
            ("cards", format!("{:?}", self.cards)),
        ]
    }
}
