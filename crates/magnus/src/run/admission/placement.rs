//! Where a tool may be used. In chat Magnus does the work itself (spec, "Chat and tasks"): a chat
//! turn reads, writes, makes files, uses its computer, starts sub-agents and
//! sends, each boundary action reviewed like any run's. A task is for work
//! on a schedule, in the background, or that outlasts the conversation.
use crate::run::kind::RunKind;
use crate::tools;

/// Why a run of this kind may not use `tool` for where it is, if so: the
/// one rule for what it is offered and what its call may do
/// (`run::admission`). In chat Magnus does the work itself, so a chat turn
/// may use any tool but a task's records; the chat tools are for chat.
pub(crate) fn placement(kind: &RunKind, tool: &str) -> Option<String> {
    let chat = kind.is_chat_turn();
    if tools::RECORDS.contains(&tool) {
        return record_placement(kind, tool);
    }
    if chat || !is_chat_only(tool) {
        return None;
    }
    if !IN_ANY_RUN.contains(&tool) {
        return Some(format!("{tool} is for chat; in a task, do the work, or suggest it to the person with suggest"));
    }
    (tool == "create_schedule" && kind.under_schedule()).then(|| "a scheduled run cannot set up another schedule: schedules would breed".to_string())
}

/// The run records: `show` is chat's, the rich-answer tools are any run's,
/// and a task's own records (plan, Done, failure, asks) are a task's. A run
/// that ends with its answer, a chat turn or a team run, has none.
fn record_placement(kind: &RunKind, tool: &str) -> Option<String> {
    if UI_RECORDS.contains(&tool) {
        return None;
    }
    if tool == "show" {
        return (!kind.is_chat_turn()).then(|| "show is for chat. A task's outcome is the deliverable of task_complete.".to_string());
    }
    if kind.is_chat_turn() {
        return Some(format!("{tool} is not available in chat. When the work is done, reply with your answer as text."));
    }
    (!kind.offers_records()).then(|| format!("{tool} is not available in a team thread. When the work is done, reply with your answer as text."))
}

/// The run records every run has: the rich-answer tools.
const UI_RECORDS: &[&str] = crate::tools_ui::RECORDS;

/// A tool of the chat family (`boundary::is_chat_tool`): the person's
/// controls. Sharing is the one bundle write that belongs to conversation,
/// as the controls do (#299): the person's say after seeing the text. A
/// suggestion is not chat-only: from a task or a review it goes to the
/// inbox instead (spec rev 140; #393).
pub(crate) fn is_chat_only(tool: &str) -> bool {
    tools::builtin(tool).is_some_and(|b| (b.app == "chat" || b.name == "bundle_share") && b.name != "suggest")
}

/// The chat tools a task or a review is offered too: a schedule never asks
/// on its own (spec rev 142, Permissions and tasks), so work outside chat
/// sets one up and sees the person's. A scheduled run sets up none:
/// schedules would breed (2026-10-30).
const IN_ANY_RUN: [&str; 2] = ["create_schedule", "list_schedules"];

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT: RunKind = RunKind::ChatTurn;
    const TASK: RunKind = RunKind::Task;

    /// Chat does the work any chat agent does: it is offered the actions
    /// that change things, the computer and sub-agents, not only reads.
    #[test]
    fn a_chat_turn_is_offered_the_work_itself() {
        for t in ["sandbox_exec", "sandbox_write_file", "deliver_file", "browser_click", "browser_type", "computer_click", "start_sub_agent", "knowledge_add", "web_fetch"] {
            assert_eq!(placement(&CHAT, t), None, "{t} is not offered in chat");
        }
        for t in ["task_contract", "task_plan", "task_complete", "task_fail"] {
            assert!(placement(&CHAT, t).is_some(), "{t}, a task's record, was offered in chat");
        }
    }

    /// A change made in chat is not refused for being in chat: it meets
    /// policy, review and approval like any run's. Only the chat tools stay
    /// in chat.
    #[test]
    fn a_chat_turn_may_change_things() {
        for t in ["sandbox_exec", "browser_click", "knowledge_add", "create_task"] {
            assert_eq!(placement(&CHAT, t), None, "{t} was refused in chat");
        }
        assert!(placement(&TASK, "create_task").is_some(), "a task used a chat tool");
        assert!(placement(&TASK, "bundle_share").is_some(), "a task shared from the bundle");
        for t in ["create_schedule", "list_schedules", "suggest"] {
            assert_eq!(placement(&TASK, t), None, "a task cannot use {t}");
            assert_eq!(placement(&RunKind::Review, t), None, "a review cannot use {t}");
        }
        assert!(placement(&TASK, "cancel_schedule").is_some());
    }

    /// A scheduled run, and a sub-agent under one, sets up no schedule:
    /// it was not offered `create_schedule`, and its call is refused at the
    /// boundary too, not only by the schedule's own check.
    #[test]
    fn a_scheduled_run_sets_up_no_schedule() {
        for kind in [RunKind::Scheduled, RunKind::SubAgent(Box::new(RunKind::Scheduled))] {
            assert!(placement(&kind, "create_schedule").is_some_and(|w| w.contains("breed")), "{kind:?}");
            assert_eq!(placement(&kind, "list_schedules"), None, "{kind:?}");
        }
    }

    /// The records: `show` in chat only, the rich-answer tools anywhere, a
    /// task's own records in no run that ends with its answer.
    #[test]
    fn records_by_kind() {
        assert_eq!(placement(&CHAT, "show"), None);
        assert!(placement(&TASK, "show").is_some());
        for kind in [CHAT, TASK, RunKind::TeamRequest, RunKind::TeamRoutine] {
            assert_eq!(placement(&kind, "ui_catalog"), None, "{kind:?}");
        }
        for kind in [RunKind::TeamRequest, RunKind::TeamRoutine] {
            assert!(placement(&kind, "task_complete").is_some(), "{kind:?} may end as a task does");
        }
        assert_eq!(placement(&TASK, "task_complete"), None);
        assert_eq!(placement(&RunKind::SubAgent(Box::new(RunKind::TeamRequest)), "task_complete"), None, "a team run's sub-agent is a task");
    }
}
