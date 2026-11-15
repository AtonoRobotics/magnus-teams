//! The inbox and the proactive loop's marks: items put in front of a
//! person (approvals, questions, notices, interruptions), when each was
//! opened and decided, the approvals given, and when reviews last woke
//! (#237, #367).

use crate::events::Ev;
use crate::forget::Erased;
use crate::log::Event;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub task: Option<String>,
    pub run: Option<String>,
    pub person: String,
    pub summary: String,
    pub details: Value,
    pub created_at: DateTime<Utc>,
    pub resolution: Option<(String, Option<String>, Option<String>, String)>,
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub id: String,
    pub person: String,
    pub task: String,
    pub tool: String,
    pub input_hash: Option<String>,
    pub scope: String,
    pub until: Option<DateTime<Utc>>,
    pub used: bool,
    pub by: String,
    /// Why the approval was asked for: the rule, or `unknown_outcome`.
    pub reason: Option<String>,
    /// The risk class the action had when it was approved (its item's): an
    /// approval covers only that class.
    pub risk: Option<String>,
    /// Where the approved action went; None for approvals from before.
    pub destinations: Option<Vec<String>>,
}

#[derive(Default, Debug)]
pub struct Inbox {
    items: BTreeMap<String, Item>,
    approvals: Vec<Approval>,
    /// When each inbox item was first presented to its person (#367).
    items_opened: HashMap<String, DateTime<Utc>>,
    /// When each inbox item was decided, for the feed (#367).
    items_resolved_at: HashMap<String, DateTime<Utc>>,
    /// The proactive loop's scheduling, by person (#237): the last review
    /// wake, the day of the last overnight review, and today's delivered
    /// interruptions (day, count).
    reviews_last: HashMap<String, DateTime<Utc>>,
    reviews_overnight: HashMap<String, String>,
    interrupts: HashMap<String, (String, u32)>,
    /// Review runs still going, run to person: one per person at a time.
    review_runs: HashMap<String, String>,
    /// One-a-day wake marks (a coming-up schedule, a due commitment).
    wake_marks: HashMap<String, String>,
}

/// Their items and those of their runs, with their opened and resolved
/// marks (#412); the approvals given for their actions, so none covers
/// whoever is next known by their name (#191; those they gave others stay,
/// as given); and the proactive loop's scheduling for them.
impl crate::forget::Forget for Inbox {
    fn forget(&mut self, who: &Erased) {
        let p = who.person.as_str();
        self.items.retain(|_, i| i.person != p && i.run.as_ref().is_none_or(|r| !who.runs.contains(r)));
        self.items_opened.retain(|id, _| !who.items.contains(id));
        self.items_resolved_at.retain(|id, _| !who.items.contains(id));
        self.approvals.retain(|a| a.person != p);
        self.reviews_last.remove(p);
        self.reviews_overnight.remove(p);
        self.interrupts.remove(p);
        self.review_runs.retain(|_, x| x != p);
        self.wake_marks.retain(|k, _| k.split_once(':').is_some_and(|(x, _)| x != p));
    }
}

impl Inbox {
    /// `Item`, `ItemOpened`, `Approval`, `ApprovalUsed` and `ReviewWoken`;
    /// `prefs` gives each person's zone, whose day an interruption and a
    /// wake count in. `State::apply` routes no other kind here.
    pub(crate) fn fold(&mut self, e: &Event, prefs: &HashMap<String, Value>) {
        match &e.ev {
            Ev::ItemOpened { id } => {
                // First presentation stands: opened is when it was
                // first seen, and the app reports it on every open.
                self.items_opened.entry(id.clone()).or_insert(e.at);
            }
            Ev::Item { id, item_kind, task, run, person, summary, details } => {
                self.items.insert(
                    id.clone(),
                    Item {
                        id: id.clone(),
                        kind: item_kind.clone(),
                        task: task.clone(),
                        run: run.clone(),
                        person: person.clone(),
                        summary: summary.clone(),
                        details: details.clone(),
                        created_at: e.at,
                        // A retired review of tool descriptions waits on no one
                        // now: definitions are screened (spec rev 140).
                        resolution: crate::consent::RETIRED_KINDS
                            .contains(&item_kind.as_str())
                            .then(|| ("superseded".into(), None, Some(crate::consent::RETIRED_TEXT.into()), "kernel".into())),
                    },
                );
                // A delivered interruption counts against the person's day
                // (#237); observations and notices never do.
                if item_kind == "interrupt" {
                    let tz = prefs.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                    let today = crate::proactive::day_key(e.at, tz);
                    self.interrupts
                        .entry(person.clone())
                        .and_modify(|(day, n)| {
                            if *day == today {
                                *n += 1
                            } else {
                                *day = today.clone();
                                *n = 1
                            }
                        })
                        .or_insert((today, 1));
                }
            }
            Ev::ReviewWoken { run, person, reason, .. } => {
                self.reviews_last.insert(person.clone(), e.at);
                self.review_runs.insert(run.clone(), person.clone());
                if reason == "overnight" {
                    let tz = prefs.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                    self.reviews_overnight.insert(person.clone(), crate::proactive::day_key(e.at, tz));
                }
                for (prefix, suffix) in [("schedule ", " coming up"), ("commitment ", " due"), ("approval ", " waiting"), ("task ", " done"), ("task ", " failed")] {
                    if let Some(id) = reason.strip_prefix(prefix).and_then(|r| r.strip_suffix(suffix)) {
                        let tz = prefs.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                        self.wake_marks.insert(format!("{person}:{prefix}{id}"), crate::proactive::day_key(e.at, tz));
                    }
                }
            }
            Ev::Approval { id, item, person, task, tool, input_hash, scope, until, reason, risk, destinations } => self.approvals.push(Approval {
                id: id.clone(),
                person: person.clone(),
                task: task.clone(),
                tool: tool.clone(),
                input_hash: input_hash.clone(),
                scope: scope.clone(),
                until: *until,
                used: false,
                by: e.actor.clone(),
                reason: reason.clone(),
                // Approvals written before the event kept it read it from
                // their item; with that deleted, they ask again.
                risk: risk.clone().or_else(|| self.items.get(item).and_then(|i| i.details["risk"].as_str()).map(String::from)),
                destinations: destinations.clone(),
            }),
            Ev::ApprovalUsed { id, .. } => {
                if let Some(a) = self.approvals.iter_mut().find(|a| &a.id == id) {
                    a.used = true;
                }
            }
            _ => {}
        }
    }

    /// `ItemResolved`: the item decided, and its run when the decision put
    /// away a notice (the run may be waiting on it).
    pub(crate) fn resolve(&mut self, e: &Event) -> Option<String> {
        let Ev::ItemResolved { id, decision, scope, text } = &e.ev else { return None };
        self.items_resolved_at.insert(id.clone(), e.at);
        let i = self.items.get_mut(id)?;
        i.resolution = Some((decision.clone(), scope.clone(), text.clone(), e.actor.clone()));
        if decision == "dismiss" {
            i.run.clone()
        } else {
            None
        }
    }

    /// A run ended: a review it was is no longer going.
    pub(crate) fn run_ended(&mut self, run: &str) {
        self.review_runs.remove(run);
    }

    /// A task going to the trash takes its items with it.
    pub(crate) fn take_items_of(&mut self, task: &str) -> Vec<Item> {
        let ids: Vec<String> = self.items.values().filter(|i| i.task.as_deref() == Some(task)).map(|i| i.id.clone()).collect();
        ids.iter().filter_map(|id| self.items.remove(id)).collect()
    }

    /// And a restored one brings them back.
    pub(crate) fn put_item(&mut self, i: Item) {
        self.items.insert(i.id.clone(), i);
    }

    pub fn items(&self) -> &BTreeMap<String, Item> {
        &self.items
    }

    /// The run's items still waiting on a decision.
    pub fn open_for_run<'a>(&'a self, run: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items.values().filter(move |i| i.run.as_deref() == Some(run) && i.resolution.is_none())
    }

    pub fn approvals(&self) -> &Vec<Approval> {
        &self.approvals
    }

    pub fn items_opened(&self) -> &HashMap<String, DateTime<Utc>> {
        &self.items_opened
    }

    pub fn items_resolved_at(&self) -> &HashMap<String, DateTime<Utc>> {
        &self.items_resolved_at
    }

    pub fn reviews_last(&self) -> &HashMap<String, DateTime<Utc>> {
        &self.reviews_last
    }

    pub fn reviews_overnight(&self) -> &HashMap<String, String> {
        &self.reviews_overnight
    }

    pub fn interrupts(&self) -> &HashMap<String, (String, u32)> {
        &self.interrupts
    }

    pub fn review_runs(&self) -> &HashMap<String, String> {
        &self.review_runs
    }

    pub fn wake_marks(&self) -> &HashMap<String, String> {
        &self.wake_marks
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("items", format!("{:?}", self.items)),
            ("items_opened", format!("{:?}", self.items_opened)),
            ("items_resolved_at", format!("{:?}", self.items_resolved_at)),
            ("approvals", format!("{:?}", self.approvals)),
            ("reviews_last", format!("{:?}", self.reviews_last)),
            ("reviews_overnight", format!("{:?}", self.reviews_overnight)),
            ("interrupts", format!("{:?}", self.interrupts)),
            ("review_runs", format!("{:?}", self.review_runs)),
            ("wake_marks", format!("{:?}", self.wake_marks)),
        ]
    }
}
