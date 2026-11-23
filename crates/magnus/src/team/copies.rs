//! Where a person's words are copied, in one place, so erasing them and
//! deleting one message for everyone reach every copy
//! (decisions/2026-11-23-one-owner-for-words-and-their-copies.md).
//!
//! A channel message's words are read again, never copied, by:
//! - a whisper, which reads its thread as it stands now (`whisper_thread`):
//!   a message deleted since is not in it.
//!
//! They were copied, and still are by events older releases wrote, into:
//! - a whisper's or a team run's `history`, its thread when it was asked.
//!   The row goes with any message it quoted; a team run's copy leaves the
//!   state with it.
//! - the mission a message asked for: its goal and its lanes' objectives,
//!   the asker's words. Deleting the message takes them.
//! - a mission's deliverable (`DeliverableUpdated`'s markdown), which quoted
//!   every released lane. It is read from the lanes now
//!   (`Mission::deliverable`); the old rows go with any lane's words.
//!
//! Erasing a person deletes each of their messages as their own delete does,
//! so the first two follow; and a mission's goal, objectives and
//! deliverable are its requester's (`MissionLedger::content_owner`).
//!
//! The events found here are named in the erasure's or the delete's
//! `Removed`, so their content leaves the log and the backups, and
//! `State::drop_copies` takes out what they put in the state when that
//! `Removed` is applied, live and on every replay. A lane stored whole
//! before 2026-11-22 keeps its objective in the record until `migrate`
//! moves it out: the replayed `Removed` is what hides it until then.
use super::missions::MissionLedger;
use super::whisper::HISTORY_MESSAGES;
use super::{Change, Ledger, Message};
use crate::events::TeamMissionEvent;
use crate::kernel::Kernel;
use crate::state::State;
use std::collections::{BTreeMap, BTreeSet};

/// Which of a mission's words an event holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Words {
    Goal,
    /// A lane's objective.
    Objective(String),
    Deliverable,
}

/// The team ledger's part: when each whisper was asked, and the rows that
/// copied a thread.
#[derive(Default, Clone, Debug)]
pub(crate) struct TeamCopies {
    /// A whisper's task, then its event: its thread is what was written before.
    whispers: BTreeMap<String, i64>,
    /// An event that copied its thread into `history`, then its workspace,
    /// channel and task.
    quoted: BTreeMap<i64, (String, String, String)>,
}

impl TeamCopies {
    pub(super) fn note(&mut self, c: &Change, seq: i64) {
        let (workspace, channel, task, history) = match c {
            Change::Whisper { workspace, channel, task, history, .. } => {
                self.whispers.insert(task.clone(), seq);
                (workspace, channel, task, history)
            }
            Change::Run { workspace, channel, task, history, .. } => (workspace, channel, task, history),
            _ => return,
        };
        if !history.is_empty() {
            self.quoted.insert(seq, (workspace.clone(), channel.clone(), task.clone()));
        }
    }
}

impl Ledger {
    /// A whisper's owner and its thread as it stands now: up to the last
    /// `HISTORY_MESSAGES` messages written before it, without those deleted
    /// since. Nothing of the thread is kept with the whisper.
    pub fn whisper_thread(&self, task: &str) -> Option<(String, String)> {
        let w = self.whispers.get(task)?;
        let asked = *self.copies.whispers.get(task)?;
        let ws = self.workspaces.get(&w.workspace)?;
        let before = |m: &&Message| self.message_seqs(&m.id).first().is_some_and(|s| *s < asked);
        let thread = w.thread.as_ref();
        let here: Vec<&Message> =
            ws.channels.get(&w.channel)?.messages.iter().filter(|m| thread.is_none() || m.thread.as_ref() == thread || Some(&m.id) == thread).filter(before).collect();
        let shown = here[here.len().saturating_sub(HISTORY_MESSAGES)..].iter().filter(|m| !ws.chat.unsent.contains(&m.id) && !m.text.is_empty());
        Some((w.owner.clone(), shown.map(|m| format!("{}: {}", m.sender, m.text)).collect::<Vec<_>>().join("\n")))
    }
}

impl MissionLedger {
    /// `apply`, noting which events hold a mission's words.
    pub fn apply_at(&mut self, ev: &TeamMissionEvent, seq: i64) {
        let words = match ev {
            TeamMissionEvent::Started { mission, .. } => Some((mission, Words::Goal)),
            TeamMissionEvent::LaneDispatched { mission, lane, .. } => Some((mission, Words::Objective(lane.clone()))),
            TeamMissionEvent::DeliverableUpdated { mission, .. } => Some((mission, Words::Deliverable)),
            _ => None,
        };
        self.apply(ev);
        if let Some((mission, words)) = words.filter(|(m, _)| self.missions.contains_key(*m)) {
            self.rows.insert(seq, (mission.clone(), words));
        }
    }

    /// The events that hold these missions' words, or, with `only`, those
    /// words alone.
    fn rows_of(&self, missions: &BTreeSet<&str>, only: Option<&Words>) -> Vec<i64> {
        self.rows.iter().filter(|(_, (m, w))| missions.contains(m.as_str()) && only.is_none_or(|o| o == w)).map(|(seq, _)| *seq).collect()
    }
}

impl State {
    /// The missions a message asked for: the lanes started on it.
    fn asked_by(&self, workspace: &str, id: &str) -> BTreeSet<&str> {
        let Some(w) = self.team.workspace(workspace) else { return BTreeSet::new() };
        let tasks: BTreeSet<&String> = w.chat.asked.iter().filter(|(_, m)| *m == id).map(|(t, _)| t).collect();
        let started = |m: &&super::missions::Mission| m.workspace == workspace && m.lanes.values().any(|l| l.child_task.as_ref().is_some_and(|t| tasks.contains(&t)));
        self.missions.missions.values().filter(started).map(|m| m.id.as_str()).collect()
    }

    /// The events beyond a person's own content that hold copies of their
    /// words: each mission they asked for (a lane stored whole, its record
    /// included), and the deliverable of each mission a lane of theirs, or
    /// released by them, is in.
    pub(crate) fn copies_of_person(&self, person: &str) -> Vec<i64> {
        let missions = self.missions.missions.values();
        let asked: BTreeSet<&str> = missions.clone().filter(|m| m.requester == person).map(|m| m.id.as_str()).collect();
        let theirs = |l: &super::missions::Lane| l.owner.as_deref() == Some(person) || l.released_by.as_deref() == Some(person);
        let in_it: BTreeSet<&str> = missions.filter(|m| m.lanes.values().any(theirs)).map(|m| m.id.as_str()).collect();
        let mut seqs = self.missions.rows_of(&asked, None);
        seqs.extend(self.missions.rows_of(&in_it, Some(&Words::Deliverable)));
        seqs
    }

    /// Events whose content was removed: what each copied into the state
    /// goes with it. Applied with `Removed`, so it holds live as after a
    /// replay, a row whose record kept the words included.
    pub(crate) fn drop_copies(&mut self, seqs: &[i64]) {
        for seq in seqs {
            if let Some((_, _, task)) = self.team.copies.quoted.remove(seq) {
                if let Some(r) = self.team.runs.get_mut(&task) {
                    r.history.clear();
                }
            }
            let Some((mission, words)) = self.missions.rows.get(seq).cloned() else { continue };
            let Some(m) = self.missions.missions.get_mut(&mission) else { continue };
            match words {
                Words::Goal => m.goal.clear(),
                Words::Objective(lane) => m.lanes.get_mut(&lane).into_iter().for_each(|l| l.objective.clear()),
                Words::Deliverable => {}
            }
        }
    }
}

impl Kernel {
    /// The events beyond its own that hold a copy of this message's words:
    /// the mission it asked for, and each row older releases wrote that
    /// quoted it, as it was sent or as it was edited. Asked before the
    /// message is deleted, while its words are still known.
    pub(crate) async fn copies_of_message(&self, workspace: &str, channel: &str, m: &Message) -> anyhow::Result<Vec<i64>> {
        let (mut seqs, quoting, own) = self.read(|s| {
            let asked = s.asked_by(workspace, &m.id);
            let rows = s.missions.rows_of(&asked, None);
            let quoting: Vec<i64> = s.team.copies.quoted.iter().filter(|(_, (w, c, _))| w == workspace && c == channel).map(|(seq, _)| *seq).collect();
            (rows, quoting, s.team.message_seqs(&m.id).to_vec())
        });
        if quoting.is_empty() || m.id.is_empty() {
            return Ok(seqs);
        }
        let mut texts = vec![m.text.clone()];
        for body in self.log.contents(&own).await?.values() {
            texts.extend([&body["message"]["text"], &body["chat"]["text"]].into_iter().filter_map(|t| t.as_str()).map(String::from));
        }
        let lines: Vec<String> = texts.iter().filter(|t| !t.trim().is_empty()).map(|t| format!("{}: {t}", m.sender)).collect();
        let quoted = self.log.contents(&quoting).await?;
        seqs.extend(quoted.into_iter().filter(|(_, b)| b["history"].as_str().is_some_and(|h| lines.iter().any(|l| h.contains(l.as_str())))).map(|(seq, _)| seq));
        Ok(seqs)
    }
}
