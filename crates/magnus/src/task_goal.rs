//! A task from chat takes a one-line goal (spec rev 138, Tasks). The
//! server carries the conversation it came from into the task, so the model
//! never restates it, and the rules the task works under are the server's
//! to enforce, not the model's to list.

use serde_json::{json, Value};

/// The longest goal `create_task` takes, in characters: one sentence.
pub const GOAL_MAX_CHARS: usize = 200;

/// The goal a `create_task` call gives, checked at the boundary: one line of
/// at most [`GOAL_MAX_CHARS`]. A call recorded before rev 138 gave the task
/// as `text`; that is read as its goal, under the same rule.
pub fn goal_of(input: &Value) -> Result<String, String> {
    one_line(input["goal"].as_str().or_else(|| input["text"].as_str()).unwrap_or_default())
}

/// A goal, trimmed, if it is one line of at most [`GOAL_MAX_CHARS`]: what
/// a task, a schedule's runs or a suggestion set out to do.
pub fn one_line(goal: &str) -> Result<String, String> {
    let goal = goal.trim();
    if goal.is_empty() {
        return Err("a goal is needed: one line saying what the work is to achieve.".into());
    }
    if goal.contains(['\n', '\r']) || goal.chars().count() > GOAL_MAX_CHARS {
        return Err(format!(
            "Nothing was started: a goal is one line of at most {GOAL_MAX_CHARS} characters. The work carries this conversation, so give the goal in one sentence and do not restate it."
        ));
    }
    Ok(goal.to_string())
}

/// The conversation a task from chat carries: the thread so far, the
/// person's message that started it (none for an opener's or an outcome's
/// turn, whose prompt is the server's), and what Magnus said in that turn
/// before starting it, so the task sees what the goal leaves out.
pub fn carried(mut conversation: Vec<Value>, asked: Option<String>, said: &str) -> Vec<Value> {
    if let Some(asked) = asked {
        conversation.push(json!({ "role": "person", "text": asked }));
    }
    let said = said.trim();
    if !said.is_empty() {
        conversation.push(json!({ "role": "you", "text": said }));
    }
    conversation
}

/// The block of a task's context that gives it the chat it came from: its
/// goal, and the conversation as recorded (`transcript`), which says what
/// the person wants in their own words.
pub fn from_chat(goal: &str, transcript: &str) -> String {
    format!(
        "This task was started from a chat with the person, to this goal: {goal}\n\nThe conversation it came from follows; the goal does not restate it. Write the task's contract from what the person said there:\n\n{transcript}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A goal is one line: several lines, or more than a sentence's length,
    /// are refused with a word that the conversation is already carried.
    #[test]
    fn a_multi_line_or_long_goal_is_refused() {
        assert_eq!(goal_of(&json!({ "goal": "  Compare LLC and S-corp for Ada. " })).as_deref(), Ok("Compare LLC and S-corp for Ada."));
        for bad in [json!({ "goal": "Compare LLC and S-corp.\nThe person said: ..." }), json!({ "goal": "a\r\nb" }), json!({ "goal": "x".repeat(GOAL_MAX_CHARS + 1) })] {
            let why = goal_of(&bad).expect_err("a long goal was taken");
            assert!(why.contains("one line") && why.contains("carries this conversation"), "{why}");
        }
        assert!(goal_of(&json!({ "goal": "é".repeat(GOAL_MAX_CHARS) })).is_ok(), "the limit counts bytes, not characters");
        assert!(goal_of(&json!({ "title": "t" })).is_err(), "no goal was taken");
        // A call recorded before the goal gave its task as text: it is read as the goal, under the same rule.
        assert_eq!(goal_of(&json!({ "title": "t", "text": "Email bob hello." })).as_deref(), Ok("Email bob hello."));
        assert!(goal_of(&json!({ "text": "Email bob.\n\nNever send anything else." })).is_err(), "the old field slipped past the rule");
    }

    /// The task's run sees the conversation it came from: earlier turns, the
    /// message that started it, and what Magnus said in that turn, with the
    /// goal named beside it.
    #[test]
    fn the_task_context_carries_the_originating_conversation() {
        let before = vec![json!({ "role": "person", "text": "We are a two-person consultancy in Ohio." }), json!({ "role": "you", "text": "Noted." })];
        let c = carried(before, Some("Which entity should we be?".into()), "I'll compare LLC and S-corp for you.");
        let roles: Vec<&str> = c.iter().filter_map(|m| m["role"].as_str()).collect();
        assert_eq!(roles, ["person", "you", "person", "you"]);
        let transcript = c.iter().map(|m| format!("{}: {}", m["role"].as_str().unwrap(), m["text"].as_str().unwrap())).collect::<Vec<_>>().join("\n\n");
        let block = from_chat("Compare LLC and S-corp.", &transcript);
        for said in ["two-person consultancy in Ohio", "Which entity should we be?", "I'll compare LLC and S-corp", "goal: Compare LLC and S-corp."] {
            assert!(block.contains(said), "the task's context lost {said:?}: {block}");
        }
        // An outcome's turn has no message of the person's, and silence adds nothing.
        assert!(carried(vec![], None, "  ").is_empty());
    }

    /// create_task's schema has one goal, a string, beside its title and
    /// grant, and no long text; its description is short, says one line and
    /// that the conversation is carried, and lists no prohibitions.
    #[test]
    fn create_task_takes_a_one_line_goal() {
        let spec = crate::tools::chat_specs().into_iter().find(|s| s.name == "create_task").expect("create_task is offered");
        let props = spec.input_schema["properties"].as_object().expect("properties");
        let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["goal", "grant", "title"], "{}", spec.input_schema);
        assert_eq!(props["goal"]["type"], "string");
        assert_eq!(spec.input_schema["required"], json!(["goal", "title"]));
        let d = &spec.description;
        assert!(d.len() < 900, "the description grew to {} bytes: {d}", d.len());
        assert!(d.contains("one line") && d.contains("carries this conversation") && d.contains("do not restate it"), "{d}");
        let lower = d.to_lowercase();
        for listed in ["never ", "must not", "don't", "\n-", "\n1.", "complete enough to work from alone"] {
            assert!(!lower.contains(listed), "the description lists prohibitions ({listed:?}): {d}");
        }
    }
}
