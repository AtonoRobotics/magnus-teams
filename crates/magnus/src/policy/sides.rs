//! Which evaluations a rule bears on: tool calls, task and schedule
//! control, or the loop's own actions, and a layer's rules split by them.

use anyhow::{anyhow, Result};
use cedar_policy::{ActionConstraint, PolicySet};
use std::str::FromStr;

/// The actions that control a task or a schedule rather than call a tool.
/// Each layer's rules are partitioned by the actions they name before an
/// evaluation (`partition_layer`), so a tool rule's `resource.risk` check
/// never runs against a task entity and a control rule's `context.is_owner`
/// check never runs against a tool request. A rule that names no action (an
/// unqualified `action`) bears on tool calls only: before control was
/// governed every unqualified rule could only mean tools, and the group's
/// sales policy still drafts one, so that meaning is kept. Name the control
/// actions to constrain control; the console refuses an unqualified forbid
/// that would silently miss it.
pub const CONTROL_ACTIONS: &[&str] = &["cancel_task", "pause_schedule", "resume_schedule", "edit_schedule", "cancel_schedule"];

/// The actions that call a tool rather than control a task or a schedule.
const TOOL_ACTIONS: &[&str] = &["execute", "request_approval"];

/// The proactive loop's own actions (#368): suggest (observations and
/// proposals), interrupt, and act (a loop run's tool calls). Suggest and
/// interrupt spend a person's attention, so their resource is the
/// person; acts touch apps, so theirs is the tool, as for execute.
pub const LOOP_ACTIONS: &[&str] = &["suggest", "interrupt", "act"];

/// Which evaluations a layer is partitioned for: tool calls, task and
/// schedule control, or the loop's own actions.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Side {
    Tool,
    Control,
    Loop,
}

/// True when a rule naming `uids` may bear on `side`'s requests. A rule
/// is dropped from a partition only when every action it names is known
/// to belong to another side; a mixed rule, or one naming an unknown
/// action, stays everywhere it might bear.
fn rule_touches(uids: &[cedar_policy::EntityUid], side: Side) -> bool {
    if uids.is_empty() {
        return true;
    }
    let others: [&[&str]; 2] = match side {
        Side::Tool => [CONTROL_ACTIONS, LOOP_ACTIONS],
        Side::Control => [TOOL_ACTIONS, LOOP_ACTIONS],
        Side::Loop => [TOOL_ACTIONS, CONTROL_ACTIONS],
    };
    !uids.iter().all(|uid| uid.type_name().to_string() == "Action" && others.iter().any(|s| s.contains(&uid.id().unescaped())))
}

/// Which evaluations a rule bears on: tool calls, task and schedule
/// control, the loop's actions — or several, for a mixed or unknown
/// action list. Unqualified rules bear on tool calls only: before
/// control was governed every unqualified rule could only mean tools,
/// and that meaning is kept for the loop as well.
pub(crate) fn policy_sides(policy: &cedar_policy::Policy) -> (bool, bool, bool) {
    match policy.action_constraint() {
        ActionConstraint::Any => (true, false, false),
        ActionConstraint::Eq(uid) => {
            let uids = std::slice::from_ref(&uid);
            (rule_touches(uids, Side::Tool), rule_touches(uids, Side::Control), rule_touches(uids, Side::Loop))
        }
        ActionConstraint::In(uids) => (rule_touches(&uids, Side::Tool), rule_touches(&uids, Side::Control), rule_touches(&uids, Side::Loop)),
    }
}

/// Split a layer's rules into the ones that bear on `side`'s requests.
/// The subset is re-rendered and policy ids renumbered, so diagnostics
/// on a mixed layer name the rule's position within its partition.
pub(super) fn partition_layer(text: &str, side: Side) -> Result<String> {
    if side == Side::Tool && !CONTROL_ACTIONS.iter().chain(LOOP_ACTIONS.iter()).any(|a| text.contains(a)) {
        return Ok(text.to_string());
    }
    let set = PolicySet::from_str(text).map_err(|e| anyhow!("{e}"))?;
    let mut out = String::new();
    for policy in set.policies() {
        let (tools, controls, loops) = policy_sides(&policy);
        let keep = match side {
            Side::Tool => tools,
            Side::Control => controls,
            Side::Loop => loops,
        };
        if keep {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&policy.to_string());
        }
    }
    Ok(out)
}
