//! A run's words: what was said for the request a run acts on, each part
//! tagged with who wrote it, so that every check asks by name for the
//! sources it trusts (2026-11-22, one reading of a run's words). Before,
//! eight callers each worked this out and disagreed: one let a chat task's
//! model-written goal stand for the person's words.
//!
//! The words are always the requester's: a sub-agent's goal is its parent
//! model's words, never the person's. A chat task's text is the model's (the
//! turn that started it wrote it); the person's words are their own lines of
//! the conversation it carries. A schedule's text is what the person set up
//! once, not a live request. A task someone else made for the person (a
//! teammate's lane, the team agent's delegation) holds no words of theirs.
//! An opener's, an outcome's and a review's text is the server's
//! instruction.

use crate::kernel::Kernel;
use crate::state::{Run, State, Task};
use anyhow::Result;
use std::collections::BTreeSet;

/// Who wrote a part of a run's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The person themselves: a task they started, their chat message,
    /// their own lines of the conversation a chat task carries.
    Person,
    /// The model: a chat task's goal, Magnus's lines of a conversation.
    Model,
    /// A schedule's text: set up once by the person, not a live request.
    Schedule,
    /// Someone else, for the person: a teammate's lane, the team agent's delegation.
    Other,
    /// The server's own instruction: an opener's, an outcome's or a review's.
    Server,
}

/// Where a part stands in the run's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// A chat turn's thread before it: said, but not this request.
    Earlier,
    /// The conversation a chat task carries: the request it came from.
    Carried,
    /// The task's own text.
    Text,
}

/// One part of a run's words.
#[derive(Clone, Debug, PartialEq)]
pub struct Said {
    pub source: Source,
    part: Part,
    pub text: String,
}

/// The words of the request a run acts on, in the order they were said.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunWords(Vec<Said>);

impl RunWords {
    /// The parts of the request itself these sources wrote: the task's text
    /// and the conversation a chat task carries, not a turn's earlier thread
    /// (write authority resets each turn, 2026-11-20).
    pub fn request_by<'a>(&'a self, trust: &'a [Source]) -> impl Iterator<Item = &'a Said> + 'a {
        self.0.iter().filter(move |w| w.part != Part::Earlier && trust.contains(&w.source))
    }

    /// `request_by`, as text.
    pub fn asked(&self, trust: &[Source]) -> Vec<&str> {
        self.0.iter().filter(|w| w.part != Part::Earlier && trust.contains(&w.source)).map(|w| w.text.as_str()).collect()
    }

    /// `asked`, one text: the parts apart by a blank line.
    pub fn asked_text(&self, trust: &[Source]) -> String {
        self.asked(trust).join("\n\n")
    }

    /// Everything these sources said for the run, a turn's earlier thread too.
    pub fn said(&self, trust: &[Source]) -> Vec<&str> {
        self.0.iter().filter(|w| trust.contains(&w.source)).map(|w| w.text.as_str()).collect()
    }

    /// The task's own text, when one of these sources wrote it.
    pub fn text(&self, trust: &[Source]) -> Option<&str> {
        self.0.iter().find(|w| w.part == Part::Text && trust.contains(&w.source)).map(|w| w.text.as_str())
    }

    /// The addresses the person wrote themselves in the request: never a
    /// tool's result, the model's words or a schedule's text.
    pub fn named_by_person(&self) -> BTreeSet<String> {
        self.asked(&[Source::Person]).into_iter().flat_map(crate::tools::addresses_in).collect()
    }
}

/// The task whose words `run` acts on (its requester's: the top of a
/// sub-agent's parents), and whether that task is an outcome's turn.
pub fn request_of(s: &State, run: &Run) -> Option<(Task, bool)> {
    let top = s.requester_run(run);
    let task = s.tasks.get(&top.task)?;
    Some((task.clone(), s.outcomes.of(&task.task_id).is_some()))
}

/// A task's words, tagged: `text` is its text as recalled for its person,
/// `outcome_turn` whether the server wrote it as an outcome's turn.
pub fn of_task(task: &Task, outcome_turn: bool, text: &str) -> RunWords {
    let person = if task.by_other { Source::Other } else { Source::Person };
    let mut words = vec![];
    if let Some(c) = &task.chat {
        let part = if c.turn { Part::Earlier } else { Part::Carried };
        for m in &c.conversation {
            // A task result's or a card's line is the work's or the server's record, nobody's words.
            let source = match m["role"].as_str() {
                Some("person") => person,
                Some("you") => Source::Model,
                _ => continue,
            };
            if let Some(t) = m["text"].as_str().filter(|t| !t.trim().is_empty()) {
                words.push(Said { source, part, text: t.to_string() });
            }
        }
    }
    let source = if task.schedule.is_some() {
        Source::Schedule
    } else if task.by_other {
        Source::Other
    } else if task.review {
        Source::Server
    } else {
        match &task.chat {
            Some(c) if !c.turn => Source::Model,
            Some(c) if c.opener.is_some() || outcome_turn => Source::Server,
            _ => Source::Person,
        }
    };
    if !text.trim().is_empty() {
        words.push(Said { source, part: Part::Text, text: text.to_string() });
    }
    RunWords(words)
}

impl Kernel {
    /// The words of the request `run` acts on, each tagged with who wrote
    /// it; the task's text recalled for its person. A run with no task
    /// (or a sub-agent whose top run is gone) has none.
    pub async fn run_words(&self, run: &Run) -> Result<RunWords> {
        let Some((task, outcome_turn)) = self.read(|s| request_of(s, run)) else { return Ok(RunWords::default()) };
        let w = self.words_of(&task.person, task.seq).await?;
        Ok(of_task(&task, outcome_turn, w.text(task.seq)))
    }
}

#[cfg(test)]
#[path = "run_words_tests.rs"]
pub(crate) mod tests;
