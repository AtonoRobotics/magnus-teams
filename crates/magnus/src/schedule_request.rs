//! `create_schedule`: a schedule the person asks for in chat is set up at
//! once (spec rev 140, Permissions and tasks: "a schedule the person set up
//! ... authorizes the work it describes", and a schedule never asks on its
//! own). There is no card to confirm. Each run takes the server's per-task
//! budget when it fires and starts with the taint the turn had, so what it
//! does at the boundary is still reviewed against the request and the
//! person's rules, as any work is.
//!
//! The thread shows the schedule as set (a `schedule_card` entry whose
//! state is confirmed). A task or a review sets one up the same way (spec
//! rev 142: a schedule never asks on its own), finite, and its runs post in
//! the conversation the work came from; a scheduled run sets up none.

use crate::boundary::Action;
use crate::chat::derived;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::Label;
use anyhow::Result;
use serde_json::{json, Value};

impl Kernel {
    /// `create_schedule` from a chat turn, a task or a review: the schedule
    /// is set, once for the call's key, and the conversation the work came
    /// from shows it. Outside chat it must end (`until` or `max_runs`), and
    /// a scheduled run sets up none: schedules would breed (2026-10-30).
    pub(crate) async fn chat_schedule(&self, a: &Action, key: &str) -> Result<(String, bool, Option<Value>)> {
        let no = |why: String| Ok((format!("No schedule set: {why}"), true, None));
        let Some((task, run, top)) = self.read(|s| s.runs.get(&a.run).and_then(|r| Some((s.tasks.get(&r.task)?.clone(), r.clone(), s.authority(&a.run)?.clone())))) else {
            anyhow::bail!("unknown run {}", a.run)
        };
        if top.schedule.is_some() {
            return no("a scheduled run cannot set up another schedule. A retry will not help.".into());
        }
        // A call recorded before rev 140 gave each run's work as `text`.
        let goal = match crate::task_goal::one_line(a.input["goal"].as_str().or(a.input["text"].as_str()).unwrap_or_default()) {
            Ok(g) => g,
            Err(why) => return no(why),
        };
        // Lists fall back to the goal when the model gave no title.
        let title = a.input["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&goal).to_string();
        let ((every, daily), (until, max_runs)) = match crate::suggestions::timing(&a.input, self.cfg.schedules.min_every_minutes) {
            Ok(Some(t)) => t,
            Ok(None) => return no("give every_minutes or daily_at. A retry without one will not help.".into()),
            Err(why) => return no(why),
        };
        if !task.is_chat_turn() && until.is_none() && max_runs.is_none() {
            return no("outside a conversation a schedule must end: give max_runs or until. A retry without one will not help.".into());
        }
        // Its runs post in the conversation the work came from (owner, 2026-11-10).
        let thread = top.chat.as_ref().map(|c| c.thread.clone());
        let (card, id) = (derived("card", key), derived("sched", key));
        if !self.read(|s| s.schedules.contains_key(&id)) {
            let set = Ev::ScheduleSet {
                id: id.clone(),
                person: task.person.clone(),
                text: goal.clone(),
                every_minutes: every,
                daily_at: daily.clone(),
                // Each run takes the server's per-task limit when it fires.
                budget: None,
                card: Some(card.clone()),
                label: Some(Label { untrusted: run.untrusted, private: run.private }),
                title: Some(title.clone()),
                until,
                max_runs,
                // Each run keeps to what she set it up to do: its goal.
                scope: Some(goal.clone()),
                thread: thread.clone(),
            };
            let actor = format!("chat:{}", a.run);
            let mut evs = vec![(actor.as_str(), set)];
            // A schedule set up in a project's conversation is in that project.
            if let Some(project) = self.read(|s| thread.as_ref().and_then(|th| s.project_of(th))) {
                evs.push((actor.as_str(), Ev::Moved { item: id.clone(), project: Some(project) }));
            }
            self.emit_all(evs).await?;
        }
        let data = json!({
            "card": card, "schedule": id, "title": title, "text": goal, "plan": goal,
            "every_minutes": every, "daily_at": daily, "until": until, "max_runs": max_runs,
        });
        let shown = if task.is_chat_turn() { " and shown in this thread" } else { "" };
        Ok((format!("Schedule {id} ({title}) is set up: {goal}. It is in the person's Schedules{shown}."), false, Some(data)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn names(specs: Vec<magnus_adapter_proto::ToolSpec>) -> Vec<String> {
        specs.into_iter().map(|t| t.name).collect()
    }

    /// One tool for each moment (spec rev 140): create_task and
    /// create_schedule for what the person asks, suggest for Magnus's own
    /// ideas. The proposal tools are gone from every run and from the
    /// descriptions.
    #[test]
    fn the_proposal_tools_are_gone() {
        let all = [names(crate::tools::chat_specs()), names(crate::tools::builtin_specs(true, true, true, true))].concat();
        for gone in ["propose_task", "propose_schedule"] {
            assert!(!all.iter().any(|n| n == gone), "{gone} is still offered");
            assert!(crate::tools::builtin(gone).is_none(), "{gone} is still a built-in");
            assert!(!include_str!("../instructions/tools.json").contains(&format!("\"{gone}\"")), "{gone} is still described");
        }
        for prompt in [crate::run::CHAT_INSTRUCTIONS, crate::run::REVIEW_INSTRUCTIONS, crate::run::OPENER, crate::run::OPENER_APP] {
            assert!(!prompt.contains("propose_task") && !prompt.contains("propose_schedule"), "{prompt:.120}");
        }
        let suggest = crate::tools::chat_specs().into_iter().find(|t| t.name == "suggest").expect("suggest");
        assert_eq!(suggest.input_schema["required"], json!(["title", "goal", "evidence", "permissions"]));
    }

    /// A schedule the person asks for is set up at once: its tool takes a
    /// one-line goal and its timing, nothing to confirm and no scope of the
    /// model's, and its description says so.
    #[test]
    fn create_schedule_sets_up_what_was_asked() {
        let spec = crate::tools::chat_specs().into_iter().find(|t| t.name == "create_schedule").expect("create_schedule");
        let props = spec.input_schema["properties"].as_object().expect("properties");
        assert!(props.contains_key("goal") && !props.contains_key("text") && !props.contains_key("scope"), "{}", spec.input_schema);
        let d = spec.description.to_lowercase();
        assert!(d.contains("set up at once") && !d.contains("confirm"), "{d}");
        assert!(crate::run::CHAT_INSTRUCTIONS.contains("it is set up at once"), "chat.md still has the person confirm a schedule");
    }

    /// Cards and inbox items from before rev 140 read as what they are now:
    /// a task proposal or a schedule still awaiting confirmation is a
    /// suggestion; a confirmed schedule card and one set up at the person's
    /// request show as set.
    #[test]
    fn old_proposals_read_as_suggestions() {
        use crate::chat::card_kind;
        assert_eq!(card_kind(&json!({ "card": "c", "proposal": true, "text": "x" }), "open"), "suggestion_card");
        assert_eq!(card_kind(&json!({ "card": "c", "text": "x", "daily_at": "07:30" }), "open"), "suggestion_card");
        assert_eq!(card_kind(&json!({ "card": "c", "text": "x", "daily_at": "07:30" }), "declined"), "suggestion_card");
        assert_eq!(card_kind(&json!({ "card": "c", "text": "x", "daily_at": "07:30" }), "confirmed"), "schedule_card");
        assert_eq!(card_kind(&json!({ "card": "c", "schedule": "s", "text": "x" }), "confirmed"), "schedule_card");
        assert_eq!(card_kind(&json!({ "card": "c", "suggestion": true, "daily_at": "07:30" }), "confirmed"), "suggestion_card");
        assert_eq!(card_kind(&json!({ "card": "c", "grant": true }), "open"), "grant_card");
        for (old, now) in [("task_proposal", "suggestion"), ("schedule_proposal", "suggestion"), ("suggestion", "suggestion"), ("approval", "approval")] {
            assert_eq!(crate::inbox::inbox_kind(old), now);
        }
    }

    /// A contract change after work started is refused, never put to the
    /// person (spec rev 140: checking is internal), and the agent is told so.
    #[test]
    fn a_frozen_contract_is_not_put_to_the_person() {
        assert!(crate::run::FROZEN.contains("frozen") && crate::run::FROZEN.contains("task_fail"));
        assert!(!crate::run::INSTRUCTIONS.contains("request to the person"), "agent.md still sends a contract change to the person");
    }
}
