//! What kind of run this is, named once (2026-11-13-run-kind). Every
//! question the kernel asks about a run's kind (which prompt it runs
//! under, whether it ends with its answer, whether it is background work,
//! where its outcome goes) is answered here from one reading of the log,
//! instead of being re-derived from the task's flags at each call site.
//!
//! A sub-agent's kind holds the kind of the run at the top of its parents,
//! looked up through the chain as its authority is (2026-11-10, sub-agents
//! carry their parent's authority): it is background work, scheduled work
//! or team work exactly when its top run is. Its own prompt and tools are
//! a task's: it is no chat turn, and does not talk in a thread.

use crate::state::State;

/// The kind of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// The person's message, answered in its thread (an opener too).
    ChatTurn,
    /// A chat turn the server started to return an ended task's outcome
    /// (`outcomes`): the person asked nothing in it.
    OutcomeTurn,
    /// A member's request in a team thread: it talks as a chat turn does.
    TeamRequest,
    /// A team routine: the team agent's own work on its interval, which
    /// reports to its channel.
    TeamRoutine,
    /// Work the person asked for, from a conversation or elsewhere.
    Task,
    /// A schedule's run.
    Scheduled,
    /// A proactive review: Magnus's own wake.
    Review,
    /// A watch poll: a quiet read.
    Poll,
    /// A sub-agent, with the kind of the run at the top of its parents.
    SubAgent(Box<RunKind>),
}

/// Which instructions a run's system context starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    Review,
    Chat,
    Team,
    Task,
}

impl Prompt {
    pub fn text(self) -> &'static str {
        match self {
            Prompt::Review => super::REVIEW_INSTRUCTIONS,
            Prompt::Chat => super::CHAT_INSTRUCTIONS,
            Prompt::Team => super::TEAM_INSTRUCTIONS,
            Prompt::Task => super::INSTRUCTIONS,
        }
    }
}

/// Where an ended run's outcome goes back to the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeReturn {
    /// A sub-agent's: to its parent, as its tool's result.
    ToParent,
    /// A task from a conversation: a turn in that conversation.
    ToConversation,
    /// A scheduled run's: a turn in the schedule's own thread.
    ToScheduleThread,
}

impl OutcomeReturn {
    /// Whether the agent's handling of it stands in for the failure notice
    /// in the person's inbox (K24 holds through the thread). A scheduled
    /// run keeps its notice: nobody is in its thread when it fails.
    pub fn replaces_notice(self) -> bool {
        self != OutcomeReturn::ToScheduleThread
    }
}

impl RunKind {
    /// The kind of `run`, through its parent chain. A run the log does not
    /// hold reads as a plain task, which answers no to every question.
    pub fn of(s: &State, run: &str) -> RunKind {
        match s.sub_agents.top(run) {
            Some(top) => RunKind::SubAgent(Box::new(Self::own(s, &top))),
            None => Self::own(s, run),
        }
    }

    fn own(s: &State, run: &str) -> RunKind {
        let Some(t) = s.runs.get(run).and_then(|r| s.tasks.get(&r.task)) else { return RunKind::Task };
        if t.is_chat_turn() {
            return if s.outcomes.of(&t.task_id).is_some() { RunKind::OutcomeTurn } else { RunKind::ChatTurn };
        }
        if s.team.run_of(&t.task_id).is_some() {
            return if t.poll { RunKind::TeamRoutine } else { RunKind::TeamRequest };
        }
        if t.review {
            RunKind::Review
        } else if t.poll {
            RunKind::Poll
        } else if t.schedule.is_some() {
            RunKind::Scheduled
        } else {
            RunKind::Task
        }
    }

    /// A chat turn of the person's (an outcome turn too): offered the chat
    /// tools and `show`, never a task's records, and told the person's
    /// grants. A team request talks like one but is not one: its thread is
    /// the workspace's, which the person's chat tools do not serve.
    pub fn is_chat_turn(&self) -> bool {
        matches!(self, RunKind::ChatTurn | RunKind::OutcomeTurn)
    }

    /// Whether the run ends with its answer, as a chat turn does: a reply
    /// with no tool call ends it (2026-11-12, team requests talk like chat).
    pub fn converses(&self) -> bool {
        self.is_chat_turn() || matches!(self, RunKind::TeamRequest | RunKind::TeamRoutine)
    }

    /// Whether it is offered a task's records (plan, Done, failure). A run
    /// that converses ends with its answer instead. Only a chat turn has a
    /// record call refused: a team run that makes one is handled as before.
    pub fn offers_records(&self) -> bool {
        !self.converses()
    }

    /// The instructions it runs under. A sub-agent's are a task's.
    pub fn prompt(&self) -> Prompt {
        match self {
            RunKind::Review => Prompt::Review,
            _ if self.is_chat_turn() => Prompt::Chat,
            RunKind::TeamRequest | RunKind::TeamRoutine => Prompt::Team,
            _ => Prompt::Task,
        }
    }

    /// Magnus's own background work (`Task::background`), a sub-agent of
    /// it included: it reaches the person only through what it produces.
    pub fn is_background(&self) -> bool {
        match self {
            RunKind::Review | RunKind::Poll | RunKind::TeamRoutine => true,
            RunKind::SubAgent(top) => top.is_background(),
            _ => false,
        }
    }

    /// A schedule's run, or a sub-agent of one: it sets up no schedule.
    pub fn under_schedule(&self) -> bool {
        match self {
            RunKind::Scheduled => true,
            RunKind::SubAgent(top) => top.under_schedule(),
            _ => false,
        }
    }

    /// Work nobody watches as it runs, whose asks expire (`unattended`).
    pub fn unattended(&self) -> bool {
        self.under_schedule() || self.is_background()
    }

    /// Team work: a request, a routine, or a sub-agent of one. It never
    /// uses a member's own apps, notes or memory (spec rev 142).
    pub fn in_team(&self) -> bool {
        match self {
            RunKind::TeamRequest | RunKind::TeamRoutine => true,
            RunKind::SubAgent(top) => top.in_team(),
            _ => false,
        }
    }

    /// Whether its end wakes a review: not background work, not a chat
    /// turn, whose person is already there (#237), and not a sub-agent,
    /// whose end goes to its parent. An end whose outcome returns as a
    /// turn wakes none either (`State::end_wakes_review`).
    pub fn end_wakes_review(&self) -> bool {
        !self.is_background() && !self.is_chat_turn() && !matches!(self, RunKind::SubAgent(_))
    }

    /// What kind of work it is in the agents list, without its words.
    pub fn label(&self) -> &'static str {
        match self {
            RunKind::SubAgent(_) => "sub_agent",
            _ if self.is_background() => "background",
            RunKind::Scheduled => "scheduled",
            _ if self.is_chat_turn() => "chat",
            _ => "task",
        }
    }
}

impl State {
    /// The kind of `run` (`RunKind::of`).
    pub fn run_kind(&self, run: &str) -> RunKind {
        RunKind::of(self, run)
    }

    /// Where this run's end comes back to the agent, if it does: a
    /// sub-agent's to its parent; a task from a conversation still there,
    /// or a scheduled run, to its thread, when it carries the
    /// `returns_outcome` module (2026-11-09, outcomes return to the agent).
    pub fn outcome_return(&self, run: &str) -> Option<OutcomeReturn> {
        let kind = self.run_kind(run);
        if matches!(kind, RunKind::SubAgent(_)) {
            return Some(OutcomeReturn::ToParent);
        }
        let r = self.runs.get(run).filter(|r| r.config.module(crate::outcomes::MODULE))?;
        let thread = self.tasks.get(&r.task).and_then(|t| t.chat.as_ref()).filter(|c| !c.turn).map(|c| c.thread.as_str())?;
        if self.trash.contains_key(thread) || self.removed_threads.contains(thread) {
            return None;
        }
        Some(if kind == RunKind::Scheduled { OutcomeReturn::ToScheduleThread } else { OutcomeReturn::ToConversation })
    }

    /// Whether this ended run's outcome comes back to the agent as a turn in
    /// its conversation, or to its parent as a sub-agent's result. Such an
    /// end wakes no review as well: the agent already has it, and one end
    /// makes one run.
    pub fn outcome_turn_follows(&self, run: &str) -> bool {
        self.outcome_return(run).is_some()
    }

    /// Whether this run's end wakes its person's review (`RunKind::end_wakes_review`).
    pub fn end_wakes_review(&self, run: &str) -> bool {
        self.run_kind(run).end_wakes_review() && !self.outcome_turn_follows(run)
    }
}

impl crate::kernel::Kernel {
    /// The kind of `run` as the log stands now.
    pub fn run_kind(&self, run: &str) -> RunKind {
        self.read(|s| RunKind::of(s, run))
    }
}

#[cfg(test)]
#[path = "kind_tests.rs"]
mod tests;
