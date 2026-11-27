//! What people did to their threads besides writing in them: entries they
//! deleted, their reactions to Magnus's messages, the disagreements Magnus
//! recorded, and the threads they branched.

use crate::events::Ev;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A disagreement Magnus recorded: what it advised, what the person decided, and later how it turned out.
#[derive(Clone, Debug, Serialize)]
pub struct Disagreement {
    pub id: String,
    pub person: String,
    pub position: String,
    pub decision: String,
    pub at: DateTime<Utc>,
    pub outcome: Option<String>,
    pub outcome_at: Option<DateTime<Utc>>,
}

/// A thread branched from another at one of its entries.
#[derive(Clone, Debug)]
pub struct Branch {
    pub person: String,
    pub from: String,
    pub upto: String,
    /// The entries up to `upto`, as they were shown then.
    pub entries: Vec<Value>,
    pub title: String,
    pub at: DateTime<Utc>,
}

/// A person's reactions, by (thread, entry): every reaction given, oldest
/// first, and when (None: taken back).
pub type Reactions = BTreeMap<(String, String), Vec<(Option<String>, DateTime<Utc>)>>;

#[derive(Default, Debug)]
pub struct ThreadExtras {
    /// Entries people deleted from their threads, by thread.
    hidden: HashMap<String, HashSet<String>>,
    /// Each person's reactions to Magnus's messages, so a run sees them as
    /// they stood when it started.
    reactions: HashMap<String, Reactions>,
    /// The disagreements Magnus recorded, by id.
    disagreements: BTreeMap<String, Disagreement>,
    /// Threads branched from another, by thread.
    branches: HashMap<String, Branch>,
}

/// What they hid, their reactions and disagreements, whose words went with
/// theirs, and their branched threads (the entries as shown then).
impl crate::forget::Forget for ThreadExtras {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let p = who.person.as_str();
        self.hidden.remove(p);
        self.hidden.retain(|t, _| !who.threads.contains(t));
        self.reactions.remove(p);
        self.disagreements.retain(|_, d| d.person != p);
        self.branches.retain(|_, b| b.person != p);
    }
}

impl ThreadExtras {
    /// `ChatHidden`, `Reacted`, `DisagreementRecorded`, `DisagreementOutcome`
    /// and `ThreadBranched`. `State::apply` routes no other kind here.
    pub(crate) fn fold(&mut self, ev: &Ev, at: DateTime<Utc>) {
        match ev {
            Ev::ChatHidden { thread, entries, .. } => {
                self.hidden.entry(thread.clone()).or_default().extend(entries.iter().cloned());
            }
            Ev::Reacted(crate::events::ReactedPayload { person, thread, entry, reaction }) => {
                let mine = self.reactions.entry(person.clone()).or_default();
                mine.entry((thread.clone(), entry.clone())).or_default().push((reaction.clone(), at));
            }
            Ev::DisagreementRecorded(crate::events::DisagreementRecordedPayload { id, person, position, decision, .. }) => {
                let d = Disagreement { id: id.clone(), person: person.clone(), position: position.clone(), decision: decision.clone(), at, outcome: None, outcome_at: None };
                self.disagreements.insert(id.clone(), d);
            }
            Ev::DisagreementOutcome(crate::events::DisagreementOutcomePayload { id, outcome, .. }) => {
                // The first outcome stands.
                if let Some(d) = self.disagreements.get_mut(id).filter(|d| d.outcome.is_none()) {
                    d.outcome = Some(outcome.clone());
                    d.outcome_at = Some(at);
                }
            }
            Ev::ThreadBranched(crate::events::ThreadBranchedPayload { person, thread, from, upto, entries, title }) => {
                self.branches.insert(thread.clone(), Branch { person: person.clone(), from: from.clone(), upto: upto.clone(), entries: entries.clone(), title: title.clone(), at });
            }
            _ => {}
        }
    }

    /// An entry removed from the trash for good: hidden from its thread.
    pub(crate) fn hide(&mut self, thread: String, entry: String) {
        self.hidden.entry(thread).or_default().insert(entry);
    }

    /// A branched thread going to the trash takes its branch with it.
    pub(crate) fn take_branch(&mut self, thread: &str) -> Option<Branch> {
        self.branches.remove(thread)
    }

    /// And a restored one brings it back.
    pub(crate) fn put_branch(&mut self, thread: String, b: Branch) {
        self.branches.insert(thread, b);
    }

    /// The entries deleted from a thread.
    pub fn hidden(&self, thread: &str) -> Option<&HashSet<String>> {
        self.hidden.get(thread)
    }

    /// Each person's reactions, by person.
    pub fn reactions(&self) -> &HashMap<String, Reactions> {
        &self.reactions
    }

    pub fn disagreements(&self) -> &BTreeMap<String, Disagreement> {
        &self.disagreements
    }

    pub fn branches(&self) -> &HashMap<String, Branch> {
        &self.branches
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("hidden", format!("{:?}", self.hidden)),
            ("reactions", format!("{:?}", self.reactions)),
            ("disagreements", format!("{:?}", self.disagreements)),
            ("branches", format!("{:?}", self.branches)),
        ]
    }
}
