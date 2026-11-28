//! A run's words: what was said for the request a run acts on, each part
//! tagged with who wrote it, so that every check asks by name for the
//! sources it trusts (2026-11-22, one reading of a run's words). Before,
//! eight callers each worked this out and disagreed: one let a chat task's
//! model-written goal stand for the person's words.
//!
//! The words are always the requester's: a sub-agent's goal is its parent
//! model's words, never the person's. A chat task's text is the model's (the
//! turn that started it wrote it); only the source turn's user message can
//! authorize delegated work. Earlier conversation supplies evidence. A schedule's text is what the person set up
//! once, not a live request, when they set it up themselves; one a run set
//! (`create_schedule`) is the model's words (2026-11-23, who wrote a
//! schedule). A task someone else made for the person (a
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
    /// or the originating message of a delegated chat task.
    Person,
    /// The model: a chat task's goal, Magnus's lines of a conversation, the
    /// text of a schedule a run set.
    Model,
    /// A schedule's text the person set up themselves (in the app, or by
    /// accepting a card): set up once, not a live request.
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
    /// The exact originating request of a task delegated by chat.
    Origin,
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
    /// and its originating request, excluding earlier conversation
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

/// The request a run acts on: its requester's task (the top of a
/// sub-agent's parents), whether that task is an outcome's turn, and who
/// wrote the text of the schedule it fires for, if any.
#[derive(Clone, Debug)]
pub struct Request {
    pub task: Task,
    outcome_turn: bool,
    schedule: Option<Source>,
    origin_seq: Option<i64>,
}

/// The task whose words `run` acts on (its requester's: the top of a
/// sub-agent's parents), and what else says who wrote them.
pub fn request_of(s: &State, run: &Run) -> Option<Request> {
    let top = s.requester_run(run);
    let task = s.tasks.get(&top.task)?;
    let schedule = task.schedule.as_deref().map(|id| schedule_source(s, id));
    let origin_seq = task.chat.as_ref().filter(|c| c.delegated).and_then(|_| delegated_request(s, task)).map(|t| t.seq);
    Some(Request { task: task.clone(), outcome_turn: s.outcomes.of(&task.task_id).is_some(), schedule, origin_seq })
}

/// Follow server-recorded delegation and retry links back to a user message.
fn delegated_request<'a>(s: &'a State, task: &'a Task) -> Option<&'a Task> {
    let thread = &task.chat.as_ref()?.thread;
    let mut task = task;
    loop {
        let chat = task.chat.as_ref()?;
        if task.by_other || task.schedule.is_some() || task.review || &chat.thread != thread {
            return None;
        }
        let prev = if !chat.turn && chat.delegated {
            let r = s.runs.get(chat.from_run.as_deref()?)?;
            s.tasks.get(&r.task)?
        } else if chat.turn {
            if let Some(of) = s.outcomes.of(&task.task_id) {
                s.tasks.get(of)?
            } else {
                let id = format!("m:{}", task.task_id);
                if chat.opener.is_some() || s.thread_extras.hidden(thread).is_some_and(|h| h.contains(&id)) || s.trash.gone(&id) {
                    return None;
                }
                return Some(task);
            }
        } else {
            return None;
        };
        if prev.person != task.person || prev.seq >= task.seq {
            return None;
        }
        task = prev;
    }
}

/// Who wrote a schedule's text: its person, when they set it up themselves
/// (in the app, or by accepting a card or a suggestion); otherwise the
/// model, whose run set it with `create_schedule`. A schedule no longer
/// known is nobody's set-up: the model's.
pub fn schedule_source(s: &State, schedule: &str) -> Source {
    if s.arrangements.schedules().get(schedule).is_some_and(|sc| sc.by_person) {
        Source::Schedule
    } else {
        Source::Model
    }
}

/// The schedule the person set up themselves that `run` fires for (at the
/// top of its parents), when its person is the run's: what may stand for
/// their approval of what it does (`Kernel::what_covers`). None for a
/// schedule a run set.
pub fn person_schedule(s: &State, run: &Run) -> Option<String> {
    let id = s.authority(&run.id)?.schedule.clone()?;
    let sc = s.arrangements.schedules().get(&id)?;
    (sc.person == run.person && schedule_source(s, &id) == Source::Schedule).then_some(id)
}

/// A request's words, tagged: `text` is its task's text as recalled for
/// its person.
pub fn of_task(r: &Request, text: &str) -> RunWords {
    let task = &r.task;
    let person = if task.by_other { Source::Other } else { Source::Person };
    let mut words = vec![];
    if let Some(c) = &task.chat {
        for m in &c.conversation {
            // A task result's or a card's line is the work's or the server's record, nobody's words.
            let source = match m["role"].as_str() {
                Some("person") => person,
                Some("you") => Source::Model,
                _ => continue,
            };
            if let Some(t) = m["text"].as_str().filter(|t| !t.trim().is_empty()) {
                words.push(Said { source, part: Part::Earlier, text: t.to_string() });
            }
        }
    }
    let source = if let Some(by) = r.schedule {
        by
    } else if task.by_other {
        Source::Other
    } else if task.review {
        Source::Server
    } else {
        match &task.chat {
            Some(c) if !c.turn && !c.selected_message => Source::Model,
            Some(c) if c.opener.is_some() || r.outcome_turn => Source::Server,
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
        let Some(r) = self.read(|s| request_of(s, run)) else { return Ok(RunWords::default()) };
        let w = self.words(std::iter::once(r.task.seq).chain(r.origin_seq).map(|seq| (r.task.person.clone(), seq))).await?;
        let mut words = of_task(&r, w.text(r.task.seq));
        append_origin(&mut words, r.origin_seq.map(|seq| w.text(seq)));
        Ok(words)
    }

    pub(crate) async fn originating_request(&self, task: &Task) -> Result<Option<String>> {
        let seq = self.read(|s| {
            task.chat.as_ref().filter(|c| c.delegated || s.outcomes.of(&task.task_id).is_some())?;
            delegated_request(s, task).map(|t| t.seq)
        });
        let Some(seq) = seq else { return Ok(None) };
        let words = self.words_of(&task.person, seq).await?;
        Ok(words.opt(seq, "text"))
    }

    /// Quotes can refer to what the model saw in conversation, separately from authority.
    pub(crate) async fn run_words_with_history(&self, run: &Run) -> Result<RunWords> {
        let mut words = self.run_words(run).await?;
        let task = self.read(|s| request_of(s, run).map(|r| r.task));
        if let Some(task) = task.filter(|t| t.chat.as_ref().is_some_and(|c| c.history.is_some())) {
            for entry in self.recent_thread_history(&task, run).await? {
                let source = match entry["role"].as_str() {
                    Some("person") => Source::Person,
                    Some("you") => Source::Model,
                    _ => continue,
                };
                if let Some(text) = entry["text"].as_str() {
                    words.0.push(Said { source, part: Part::Earlier, text: text.to_string() });
                }
            }
        }
        Ok(words)
    }
}

fn append_origin(words: &mut RunWords, text: Option<&str>) {
    if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
        words.0.push(Said { source: Source::Person, part: Part::Origin, text: text.to_string() });
    }
}

#[cfg(test)]
#[path = "run_words_tests.rs"]
pub(crate) mod tests;
