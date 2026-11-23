//! A message sent again, edited or an answer retried, as one chat turn
//! (decisions/2026-11-23-a-message-sent-again-is-admitted-first.md). The new
//! turn is admitted first: a switch, the person's model access or the
//! month's spend limit refuses it before anything changes. Then the new
//! turn, the hiding of the message and what came of it, and the end of the
//! work that message started are written together, and only the thread's
//! own work ends.

use crate::chat_thread::{Entry, Kind};
use crate::events::{ChatOrigin, Ev};
use crate::kernel::{Admitted, Kernel, NewRun};
use crate::state::State;
use crate::tools::Label;
use std::collections::HashSet;
use std::sync::Arc;

/// How a message goes again: its words, the model that answered it, the
/// files it had, and the thread's project.
pub(crate) struct Again {
    pub text: String,
    pub model: Option<(String, String)>,
    pub files: Vec<String>,
    pub project: Option<String>,
}

impl Kernel {
    /// Send the message at `message` in `entries` (the person's thread
    /// `thread` as it shows) again: it and everything after it leave the
    /// thread, the work it started stops, and a new turn answers it with the
    /// conversation before it. The new turn's task, or the refusal, with
    /// nothing changed.
    pub(crate) async fn send_again(self: &Arc<Self>, person: &str, thread: &str, entries: &[Entry], message: usize, again: Again) -> anyhow::Result<String> {
        // The message and what came of it: its turns, and what those turns
        // started. An earlier turn's task result that happens to end later
        // stays: it answers a message still shown.
        let later = &entries[message..];
        let turns: HashSet<String> =
            later.iter().filter(|e| matches!(e.kind, Kind::Message | Kind::Answer | Kind::Turn | Kind::ScheduleCard)).filter_map(|e| e.task_id.clone()).collect();
        let gone: Vec<String> = later
            .iter()
            .filter(|e| match self.origin_turn(e) {
                Some(t) => turns.contains(&t),
                None => true,
            })
            .filter(|e| e.has_id())
            .map(|e| e.id.clone())
            .collect();
        // The new turn reads the thread as it will show, and carries the
        // taint of the runs that wrote that (I2).
        let shown: Vec<Entry> = entries.iter().filter(|e| !(e.has_id() && gone.contains(&e.id))).cloned().collect();
        let label = self.label_of(&shown);
        let o = NewRun {
            chat: Some(ChatOrigin { thread: thread.into(), turn: true, from_run: None, conversation: Self::transcript_of(&shown), opener: None }),
            label: (label != Label::PRINCIPAL).then_some(label),
            model: again.model,
            files: again.files,
            project: again.project,
            ..Default::default()
        };
        let actor = format!("person:{person}");
        let Admitted { task, run, actor: by, evs: started } = self.admit_new(&actor, person, again.text.clone(), vec![], o).await?;
        // Superseded work stops: a turn still running or waiting, and the
        // tasks it started, would otherwise act on a message no longer
        // shown, and the new turn may start them again.
        let stop = self.read(|s| superseded(s, thread, &turns));
        let mut evs: Vec<(&str, Ev)> = vec![(actor.as_str(), Ev::ChatHidden { person: person.into(), thread: thread.into(), entries: gone })];
        evs.extend(stop.iter().map(|r| (actor.as_str(), Ev::Cancelled { run: r.clone(), source: Some("button".into()), turn: None })));
        evs.extend(started.into_iter().map(|e| (by.as_str(), e)));
        self.cancel_trees(&stop, evs).await?;
        self.wake(&run);
        self.tell_working(person, thread, &task, &again.text).await;
        Ok(task)
    }
}

/// The runs still going that `turns` started in `thread`: the turns, and
/// the tasks they started. A branch keeps the task ids of the thread it was
/// made from; the work going on there is not the branch's to stop.
fn superseded(s: &State, thread: &str, turns: &HashSet<String>) -> Vec<String> {
    s.tasks
        .values()
        .filter(|t| t.chat.as_ref().is_some_and(|c| c.thread == thread))
        .filter(|t| turns.contains(&t.task_id) || t.chat.as_ref().and_then(|c| c.from_run.as_ref()).and_then(|r| s.runs.get(r)).is_some_and(|r| turns.contains(&r.task)))
        .filter_map(|t| s.runs.get(&t.run_id))
        .filter(|r| !r.status.terminal())
        .map(|r| r.id.clone())
        .collect()
}

#[cfg(test)]
#[path = "chat_turn_tests.rs"]
mod tests;
