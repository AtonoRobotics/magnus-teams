//! Where a person's words are copied, in one place, so erasing them and
//! deleting one message for everyone reach every copy
//! (decisions/2026-11-23-one-owner-for-words-and-their-copies.md).
//!
//! A channel message's words are read again, not copied, by a whisper,
//! which reads its thread as it stands now (`whisper_thread`): a message
//! deleted since is not in it. They are copied into the mission an
//! `@Magnus` message started, as its goal and its lanes' objectives, which
//! deleting the message takes.
//!
//! Events older releases wrote hold more copies:
//! - a whisper's or a team run's `history`, its thread when it was asked:
//!   the row goes with any message it quoted, and a team run's copy leaves
//!   the state with it;
//! - a mission's `DeliverableUpdated` markdown, which quoted every released
//!   lane: the deliverable is read from the lanes now
//!   (`Mission::deliverable`), and the row goes with any lane's words.
//!
//! Erasing a person deletes each of their messages as their own delete
//! does, so the copies of those follow; a mission's goal and objectives are
//! its requester's (`MissionLedger::content_owner`), and the deliverable
//! rows go with any contributor.
//!
//! A lane's objective is copied once more into each task that works on it:
//! the lane's own task and every steer of the lane, which are the lane
//! owner's tasks. They go with the requester's erasure and with the ask
//! that started the mission (`Kernel::lane_copies`). A steer is told by the
//! opening of its text (`steer_prompt`), which older releases wrote the
//! same, as nothing else ties it to its lane.
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

/// A steer's text: the lane's objective, the owner's draft and their edit.
pub(super) fn steer_prompt(objective: &str, draft: &str, instruction: &str) -> String {
    format!("{}\n\nCurrent draft:\n{draft}\n\nOwner's edit:\n{instruction}", steer_opening(objective))
}

/// How a steer of a lane with this objective begins.
fn steer_opening(objective: &str) -> String {
    format!("Revise your private draft for \"{objective}\" using the owner's instruction.")
}

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
    /// Only what is noted of these tasks stays: the rest were forgotten.
    pub(super) fn keep(&mut self, kept: impl Fn(&str) -> bool) {
        self.whispers.retain(|t, _| kept(t));
        self.quoted.retain(|_, (_, _, t)| kept(t));
    }

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

    /// The mission events beyond a person's own content that hold copies of
    /// their words: each mission they asked for (a lane stored whole, its
    /// record included), and the deliverable of each mission a lane of
    /// theirs, or released by them, is in. With the missions they asked for.
    fn mission_copies_of(&self, person: &str) -> (Vec<i64>, BTreeSet<String>) {
        let missions = self.missions.missions.values();
        let asked: BTreeSet<&str> = missions.clone().filter(|m| m.requester == person).map(|m| m.id.as_str()).collect();
        let theirs = |l: &super::missions::Lane| l.owner.as_deref() == Some(person) || l.released_by.as_deref() == Some(person);
        let in_it: BTreeSet<&str> = missions.filter(|m| m.lanes.values().any(theirs)).map(|m| m.id.as_str()).collect();
        let mut seqs = self.missions.rows_of(&asked, None);
        seqs.extend(self.missions.rows_of(&in_it, Some(&Words::Deliverable)));
        (seqs, asked.into_iter().map(String::from).collect())
    }

    /// The tasks of these missions' lanes: each lane's own, and each task
    /// that may be a steer of one, with the opening a steer's text has: its
    /// owner's, written after the lane was dispatched.
    fn lane_tasks(&self, missions: &BTreeSet<String>) -> (Vec<i64>, Vec<(i64, String)>) {
        let (mut own, mut steers) = (vec![], vec![]);
        for m in missions.iter().filter_map(|id| self.missions.missions.get(id)) {
            for l in m.lanes.values() {
                own.extend(l.child_task.as_ref().and_then(|t| self.tasks.get(t)).map(|t| t.seq));
                let Some(owner) = l.owner.as_deref().filter(|_| !l.objective.is_empty()) else { continue };
                let objective = Words::Objective(l.id.clone());
                let dispatched = self.missions.rows.iter().find(|(_, (id, w))| *id == m.id && *w == objective).map_or(0, |(seq, _)| *seq);
                let opening = steer_opening(&l.objective);
                let after = self.tasks.values().filter(|t| t.person == owner && t.seq > dispatched && l.child_task.as_ref() != Some(&t.task_id));
                steers.extend(after.map(|t| (t.seq, opening.clone())));
            }
        }
        (own, steers)
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
    /// The events beyond a person's own content that hold copies of their
    /// words: those of the missions (`State::mission_copies_of`), and the
    /// tasks of the lanes of each mission they asked for.
    pub(crate) async fn copies_of_person(&self, person: &str) -> anyhow::Result<Vec<i64>> {
        let (mut seqs, asked) = self.read(|s| s.mission_copies_of(person));
        seqs.extend(self.lane_copies(&asked).await?);
        Ok(seqs)
    }

    /// The tasks that hold these missions' objectives: each lane's own, and
    /// each steer of a lane, told by how its text begins.
    async fn lane_copies(&self, missions: &BTreeSet<String>) -> anyhow::Result<Vec<i64>> {
        let (mut seqs, maybe) = self.read(|s| s.lane_tasks(missions));
        let bodies = self.log.contents(&maybe.iter().map(|(seq, _)| *seq).collect::<Vec<_>>()).await?;
        let steer = |(seq, opening): &&(i64, String)| bodies.get(seq).and_then(|b| b["text"].as_str()).is_some_and(|t| t.starts_with(opening.as_str()));
        seqs.extend(maybe.iter().filter(steer).map(|(seq, _)| *seq));
        Ok(seqs)
    }

    /// The events beyond its own that hold a copy of this message's words:
    /// the mission it asked for and its lanes' tasks, and each row older
    /// releases wrote that quoted it, as it was sent or as it was edited.
    /// Asked before the message is deleted, while its words are still known.
    pub(crate) async fn copies_of_message(&self, workspace: &str, channel: &str, m: &Message) -> anyhow::Result<Vec<i64>> {
        let (mut seqs, asked, quoting, own) = self.read(|s| {
            let asked = s.asked_by(workspace, &m.id);
            let rows = s.missions.rows_of(&asked, None);
            let quoting: Vec<i64> = s.team.copies.quoted.iter().filter(|(_, (w, c, _))| w == workspace && c == channel).map(|(seq, _)| *seq).collect();
            let asked: BTreeSet<String> = asked.into_iter().map(String::from).collect();
            (rows, asked, quoting, s.team.message_seqs(&m.id).to_vec())
        });
        seqs.extend(self.lane_copies(&asked).await?);
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
