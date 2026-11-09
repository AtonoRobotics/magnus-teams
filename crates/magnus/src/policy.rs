//! Layered Cedar policy (E1–E4).
//!
//! Entities: `Person::"<id>"` in `Group::"<id>"`; `Tool::"<app>__<tool>"` in
//! `App::"<app>"` with attributes `risk` and `audience`. Context: `untrusted`
//! and `private`, the run's taint. Actions: `Action::"execute"` (act without
//! asking) and `Action::"request_approval"`.
//!
//! An action is permitted only if the organization layer permits it and no
//! lower layer (each of the person's groups, then the user) forbids it. Lower
//! layers can only narrow. Any evaluation error denies.

use crate::config::Person;
use crate::tools::{Audience, Risk};
use anyhow::{anyhow, Result};
use cedar_policy::{ActionConstraint, Authorizer, Context, Decision, Entities, Entity, EntityUid, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

/// The organization policy in force until the owner writes one: the agent
/// reads, makes reversible writes and uses its sandbox on its own, and asks
/// for everything else.
pub const DEFAULT_ORG: &str = r#"
permit(principal, action == Action::"execute", resource)
  when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" };
permit(principal, action == Action::"request_approval", resource);
"#;

pub struct ToolFacts<'a> {
    pub name: &'a str,
    pub app: &'a str,
    pub risk: Risk,
    pub audience: Audience,
}

/// The organization policy in force until the owner writes one: the agent
/// reads, makes reversible writes and uses its sandbox on its own, and asks
/// for everything else; and anyone controls their own tasks and schedules,
/// the owner and admins anyone's.
pub const DEFAULT_CONTROL: &str = r#"
permit(principal, action == Action::"cancel_task", resource)
  when { resource in principal || context.is_owner || context.is_admin };
permit(principal, action == Action::"pause_schedule", resource)
  when { resource in principal || context.is_owner || context.is_admin };
permit(principal, action == Action::"resume_schedule", resource)
  when { resource in principal || context.is_owner || context.is_admin };
permit(principal, action == Action::"edit_schedule", resource)
  when { resource in principal || context.is_owner || context.is_admin };
permit(principal, action == Action::"cancel_schedule", resource)
  when { resource in principal || context.is_owner || context.is_admin };
"#;

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
fn partition_layer(text: &str, side: Side) -> Result<String> {
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

/// The permit-everything rule a control evaluation adds to a lower layer,
/// so the layer denies only if one of its own forbids matched: the tool
/// boundary's unqualified blanket bears on tool calls only, so control
/// names its actions.
const CONTROL_PERMIT_ALL: &str =
    "permit(principal, action in [Action::\"cancel_task\", Action::\"pause_schedule\", Action::\"resume_schedule\", Action::\"edit_schedule\", Action::\"cancel_schedule\"], resource);";

/// One control operation: `op` on the `kind` ("Task", "Schedule") `id`
/// owned by `owner`.
pub struct ControlFacts<'a> {
    pub op: &'a str,
    pub kind: &'a str,
    pub id: &'a str,
    pub owner: &'a str,
}

fn control_entities(person: &Person, t: &ControlFacts) -> Result<Entities> {
    let mut groups = HashSet::new();
    let mut list = vec![];
    for g in &person.groups {
        let g = uid("Group", g)?;
        groups.insert(g.clone());
        list.push(Entity::new_no_attrs(g, HashSet::new()));
    }
    let who = uid("Person", &person.id)?;
    list.push(Entity::new_no_attrs(who.clone(), groups));
    if t.owner != person.id {
        list.push(Entity::new_no_attrs(uid("Person", t.owner)?, HashSet::new()));
    }
    let owner = uid("Person", t.owner)?;
    list.push(Entity::new_no_attrs(uid(t.kind, t.id)?, [owner].into_iter().collect()));
    Entities::from_entities(list, None).map_err(|e| anyhow!("{e}"))
}

/// Why a control was refused: the policy forbids it, or the policy
/// itself failed to evaluate. Callers tell the two apart: a broken policy
/// falls back to today's fixed rule, while a forbid binds even those the
/// fixed rule would let through.
pub enum ControlDeny {
    Forbidden(String),
    Broken(String),
}

/// Whether `person` may `t` under the layered policy: the organization
/// layer permits and no lower layer forbids (#266). The allowing rule, or
/// why not.
pub fn control_allowed(policies: &BTreeMap<String, String>, person: &Person, t: &ControlFacts) -> Result<String, ControlDeny> {
    let failed = |e: String| ControlDeny::Broken(format!("policy failed to evaluate {} for {}: {e}", t.op, person.id));
    let ents = control_entities(person, t).map_err(|e| failed(format!("{e:#}")))?;
    let layers = layers_for(policies, person, None);
    let ctx = Context::from_pairs([
        ("is_owner".to_string(), RestrictedExpression::new_bool(person.is_owner())),
        ("is_admin".to_string(), RestrictedExpression::new_bool(person.has_role("admin"))),
    ])
    .map_err(|e| failed(e.to_string()))?;
    let req = Request::new(
        uid("Person", &person.id).map_err(|e| failed(format!("{e:#}")))?,
        uid("Action", t.op).map_err(|e| failed(format!("{e:#}")))?,
        uid(t.kind, t.id).map_err(|e| failed(format!("{e:#}")))?,
        ctx,
        None,
    )
    .map_err(|e| failed(e.to_string()))?;
    let org = format!("{}\n{DEFAULT_CONTROL}", partition_layer(&layers[0].1, Side::Control).map_err(|e| failed(format!("{e:#}")))?);
    let (mut ok, rule) = eval(&org, "org", &req, &ents).map_err(|e| failed(format!("{e:#}")))?;
    let mut reason = rule.first().cloned();
    for (name, text) in &layers[1..] {
        let narrowed = format!("{}\n{CONTROL_PERMIT_ALL}", partition_layer(text, Side::Control).map_err(|e| failed(format!("{e:#}")))?);
        let (allows, rules) = eval(&narrowed, name, &req, &ents).map_err(|e| failed(format!("{e:#}")))?;
        if !allows {
            ok = false;
            reason = rules.first().cloned().or_else(|| Some(name.clone()));
        }
    }
    if ok {
        Ok(reason.unwrap_or_default())
    } else {
        Err(ControlDeny::Forbidden(match reason {
            Some(r) => format!("policy forbids {} for {} (rule {r})", t.op, person.id),
            None => format!("no policy permits {} for {}", t.op, person.id),
        }))
    }
}

/// The loop's actions in force until the owner writes narrower ones:
/// the loop suggests, interrupts and acts (#368). An org narrows per
/// resource from here; lower layers can only narrow further.
pub const DEFAULT_LOOP: &str = r#"
permit(principal, action in [Action::"suggest", Action::"interrupt", Action::"act"], resource);
"#;

const LOOP_PERMIT_ALL: &str = "permit(principal, action in [Action::\"suggest\", Action::\"interrupt\", Action::\"act\"], resource);";

fn loop_entities(person: &Person, tool: Option<&ToolFacts>) -> Result<Entities> {
    let mut groups = HashSet::new();
    let mut list = vec![];
    for g in &person.groups {
        let g = uid("Group", g)?;
        groups.insert(g.clone());
        list.push(Entity::new_no_attrs(g, HashSet::new()));
    }
    list.push(Entity::new_no_attrs(uid("Person", &person.id)?, groups));
    if let Some(t) = tool {
        let app = uid("App", t.app)?;
        list.push(Entity::new_no_attrs(app.clone(), HashSet::new()));
        let attrs: HashMap<String, RestrictedExpression> = [
            ("risk".to_string(), RestrictedExpression::new_string(t.risk.as_str().into())),
            ("audience".to_string(), RestrictedExpression::new_string(t.audience.as_str().into())),
        ]
        .into_iter()
        .collect();
        list.push(Entity::new(uid("Tool", t.name)?, attrs, [app].into_iter().collect()).map_err(|e| anyhow!("{e}"))?);
    }
    Entities::from_entities(list, None).map_err(|e| anyhow!("{e}"))
}

/// Why a loop output was refused: the policy forbids it, or the policy
/// itself failed to evaluate.
pub enum LoopDeny {
    Forbidden(String),
    Broken(String),
}

/// Whether the loop may `op` (`suggest`, `interrupt`, `act`) for
/// `person`: the organization layer permits and no lower layer forbids
/// (#368). Suggest and interrupt name the person as the resource whose
/// attention they spend; act names the tool, as execute does. The
/// allowing rule, or why not.
pub fn loop_allowed(policies: &BTreeMap<String, String>, person: &Person, op: &str, tool: Option<&ToolFacts>, untrusted: bool, private: bool) -> Result<String, LoopDeny> {
    let failed = |e: String| LoopDeny::Broken(format!("policy failed to evaluate {op} for {}: {e}", person.id));
    let ents = loop_entities(person, tool).map_err(|e| failed(format!("{e:#}")))?;
    let layers = layers_for(policies, person, None);
    let ctx = Context::from_pairs([("untrusted".to_string(), RestrictedExpression::new_bool(untrusted)), ("private".to_string(), RestrictedExpression::new_bool(private))])
        .map_err(|e| failed(e.to_string()))?;
    let resource = match tool {
        Some(t) => uid("Tool", t.name).map_err(|e| failed(format!("{e:#}")))?,
        None => uid("Person", &person.id).map_err(|e| failed(format!("{e:#}")))?,
    };
    let req = Request::new(uid("Person", &person.id).map_err(|e| failed(format!("{e:#}")))?, uid("Action", op).map_err(|e| failed(format!("{e:#}")))?, resource, ctx, None)
        .map_err(|e| failed(e.to_string()))?;
    let org = format!("{}\n{DEFAULT_LOOP}", partition_layer(&layers[0].1, Side::Loop).map_err(|e| failed(format!("{e:#}")))?);
    let (mut ok, rule) = eval(&org, "org", &req, &ents).map_err(|e| failed(format!("{e:#}")))?;
    let mut reason = rule.first().cloned();
    for (name, text) in &layers[1..] {
        let narrowed = format!("{}\n{LOOP_PERMIT_ALL}", partition_layer(text, Side::Loop).map_err(|e| failed(format!("{e:#}")))?);
        let (allows, rules) = eval(&narrowed, name, &req, &ents).map_err(|e| failed(format!("{e:#}")))?;
        if !allows {
            ok = false;
            reason = rules.first().cloned().or_else(|| Some(name.clone()));
        }
    }
    if ok {
        Ok(reason.unwrap_or_default())
    } else {
        Err(LoopDeny::Forbidden(match reason {
            Some(r) => format!("policy forbids {op} for {} (rule {r})", person.id),
            None => format!("no policy permits {op} for {}", person.id),
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Allow { rule: String },
    Ask { rule: String },
    Deny { reason: String, rule: Option<String> },
}

/// Parse a layer's policy text, so a bad policy is refused when written.
pub fn validate(cedar: &str) -> Result<()> {
    PolicySet::from_str(cedar).map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

fn uid(kind: &str, id: &str) -> Result<EntityUid> {
    let lit = format!("{kind}::{}", serde_json::to_string(id)?);
    EntityUid::from_str(&lit).map_err(|e| anyhow!("entity {lit}: {e}"))
}

fn entities(person: &Person, t: &ToolFacts) -> Result<Entities> {
    let mut groups = HashSet::new();
    let mut list = vec![];
    for g in &person.groups {
        let g = uid("Group", g)?;
        groups.insert(g.clone());
        list.push(Entity::new_no_attrs(g, HashSet::new()));
    }
    list.push(Entity::new_no_attrs(uid("Person", &person.id)?, groups));
    let app = uid("App", t.app)?;
    list.push(Entity::new_no_attrs(app.clone(), HashSet::new()));
    let attrs: HashMap<String, RestrictedExpression> =
        [("risk".to_string(), RestrictedExpression::new_string(t.risk.as_str().into())), ("audience".to_string(), RestrictedExpression::new_string(t.audience.as_str().into()))]
            .into_iter()
            .collect();
    list.push(Entity::new(uid("Tool", t.name)?, attrs, [app].into_iter().collect()).map_err(|e| anyhow!("{e}"))?);
    Entities::from_entities(list, None).map_err(|e| anyhow!("{e}"))
}

/// The evaluation of one policy set: whether it allows, and the rules that
/// decided it.
fn eval(cedar: &str, layer: &str, req: &Request, ents: &Entities) -> Result<(bool, Vec<String>)> {
    // Cedar refuses below 100KB of remaining stack as recursion-limit, and
    // an evaluation itself takes hundreds of KB: growing here keeps every
    // decision, for tool calls, controls and grants alike, independent of
    // how deep its caller is (#375). Below 1MB remaining (over twice the
    // measured need) the eval moves to a fresh 2MB segment; a shallower
    // caller runs inline (#377).
    stacker::maybe_grow(1024 * 1024, 2 * 1024 * 1024, || {
        let set = PolicySet::from_str(cedar).map_err(|e| anyhow!("policy layer {layer} does not parse: {e}"))?;
        let resp = Authorizer::new().is_authorized(req, &set, ents);
        let errors: Vec<String> = resp.diagnostics().errors().map(|e| e.to_string()).collect();
        if !errors.is_empty() {
            return Err(anyhow!("policy layer {layer} failed to evaluate: {}", errors.join("; ")));
        }
        let rules = resp.diagnostics().reason().map(|id| format!("{layer}:{id}")).collect();
        Ok((resp.decision() == Decision::Allow, rules))
    })
}

/// The layers that apply to `person` in `task`, top first: org, groups,
/// user, task.
pub fn layers_for(policies: &BTreeMap<String, String>, person: &Person, task: Option<&str>) -> Vec<(String, String)> {
    let mut v = vec![("org".to_string(), policies.get("org").cloned().unwrap_or_else(|| DEFAULT_ORG.to_string()))];
    for g in &person.groups {
        let k = format!("group/{g}");
        if let Some(p) = policies.get(&k) {
            v.push((k, p.clone()));
        }
    }
    let k = format!("user/{}", person.id);
    if let Some(p) = policies.get(&k) {
        v.push((k, p.clone()));
    }
    if let Some(p) = task.map(|t| format!("task/{t}")).and_then(|k| policies.get(&k).map(|p| (k, p.clone()))) {
        v.push(p);
    }
    v
}

pub fn decide(policies: &BTreeMap<String, String>, person: &Person, task: Option<&str>, tool: &ToolFacts, untrusted: bool, private: bool) -> Result<Outcome> {
    let ents = entities(person, tool)?;
    let layers = layers_for(policies, person, task);
    let ctx = Context::from_pairs([("untrusted".to_string(), RestrictedExpression::new_bool(untrusted)), ("private".to_string(), RestrictedExpression::new_bool(private))])
        .map_err(|e| anyhow!("{e}"))?;

    let mut forbidden: Option<String> = None;
    let mut results = vec![];
    for action in ["execute", "request_approval"] {
        let req = Request::new(uid("Person", &person.id)?, uid("Action", action)?, uid("Tool", tool.name)?, ctx.clone(), None).map_err(|e| anyhow!("{e}"))?;
        let (org_ok, org_rules) = eval(&partition_layer(&layers[0].1, Side::Tool)?, "org", &req, &ents)?;
        let mut ok = org_ok;
        let mut rule = org_rules.first().cloned();
        if !org_ok && !org_rules.is_empty() {
            forbidden.get_or_insert_with(|| org_rules[0].clone());
        }
        for (name, text) in &layers[1..] {
            // With a permit-everything rule added, the layer denies only if one
            // of its own forbids matched.
            let narrowed = format!("{}\npermit(principal, action, resource);", partition_layer(text, Side::Tool)?);
            let (allows, rules) = eval(&narrowed, name, &req, &ents)?;
            if !allows {
                ok = false;
                rule = rules.first().cloned();
                forbidden.get_or_insert_with(|| rules.first().cloned().unwrap_or_else(|| name.clone()));
            }
        }
        results.push((ok, rule));
    }
    let (exec_ok, exec_rule) = results[0].clone();
    let (ask_ok, ask_rule) = results[1].clone();
    Ok(if exec_ok {
        Outcome::Allow { rule: exec_rule.unwrap_or_default() }
    } else if ask_ok {
        Outcome::Ask { rule: ask_rule.unwrap_or_default() }
    } else {
        let reason = match &forbidden {
            Some(r) => format!("policy rule {r} forbids {} for {}", tool.name, person.id),
            None => format!("no policy permits {} for {}", tool.name, person.id),
        };
        Outcome::Deny { reason, rule: forbidden }
    })
}

/// A grant's policy (#235), written by the kernel from what the person
/// chose: their own actions in the grant's apps, less what stays excluded.
/// Moving money always stays excluded (the kernel cannot measure an
/// amount, so no spend limit could hold), as does anything prohibited; so
/// do irreversible actions (deleting, committing them to anything) and
/// sends (a change that reaches someone beyond the person), unless the
/// person lifted that exclusion (`allow`: `irreversible`, `send`).
pub fn grant_cedar(person: &str, apps: &[String], allow: &[String]) -> Result<String> {
    let q = |s: &str| serde_json::to_string(s).map_err(|e| anyhow!("{e}"));
    let within = apps.iter().map(|a| Ok(format!("resource in App::{}", q(a)?))).collect::<Result<Vec<_>>>()?.join(" || ");
    if within.is_empty() {
        return Err(anyhow!("a grant covers at least one app"));
    }
    let mut p = format!("permit(principal == Person::{}, action == Action::\"execute\", resource)\n  when {{ {within} }};\n", q(person)?);
    p.push_str("forbid(principal, action, resource) when { resource.risk == \"financial\" };\n");
    p.push_str("forbid(principal, action, resource) when { resource.risk == \"prohibited\" };\n");
    if !allow.iter().any(|a| a == "irreversible") {
        p.push_str("forbid(principal, action, resource) when { resource.risk == \"irreversible\" };\n");
    }
    if !allow.iter().any(|a| a == "send") {
        p.push_str("forbid(principal, action, resource) when { resource.audience != \"private\" && resource.risk != \"read\" && resource.risk != \"sandbox\" };\n");
    }
    validate(&p)?;
    Ok(p)
}

/// Whether a grant's policy lets `person` take this action without asking:
/// its own rules alone decide, evaluated as any layer is.
pub fn grant_covers(cedar: &str, person: &Person, tool: &ToolFacts) -> Result<bool> {
    let ents = entities(person, tool)?;
    let req = Request::new(uid("Person", &person.id)?, uid("Action", "execute")?, uid("Tool", tool.name)?, Context::empty(), None).map_err(|e| anyhow!("{e}"))?;
    Ok(eval(cedar, "grant", &req, &ents)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str) -> Person {
        serde_json::from_value(serde_json::json!({ "id": id, "groups": ["staff"] })).unwrap()
    }

    /// A decision deep in the stack still evaluates: Cedar refuses below
    /// 100KB of remaining stack as recursion-limit, and an evaluation
    /// itself takes hundreds of KB, so the eval grows the stack first.
    /// Without that, this decision fails (#375).
    ///
    /// Calibrated for debug builds against Cedar 4.14: the burn stops with
    /// remaining stack in [200KB, 260KB], above the overflow edge (~190KB)
    /// and below the trip edge (~400KB for the default policy), so without
    /// the grow this fails cleanly instead of aborting. The band is
    /// asserted: a miss fails loudly rather than passing silently.
    #[test]
    fn a_decision_deep_in_the_stack_still_evaluates() {
        fn deep(policies: &BTreeMap<String, String>, person: &Person, tool: &ToolFacts) -> Result<Outcome> {
            match stacker::remaining_stack() {
                // The platform cannot report it: nothing to burn.
                None => decide(policies, person, None, tool, false, false),
                Some(r) if r > 230 * 1024 => {
                    let pad = [0u8; 4096];
                    std::hint::black_box(&pad);
                    deep(policies, person, tool)
                }
                Some(at) => {
                    assert!((200 * 1024..=260 * 1024).contains(&at), "the burn missed its band: {at}");
                    decide(policies, person, None, tool, false, false)
                }
            }
        }
        let policies = BTreeMap::new();
        let tool = ToolFacts { name: "mail__list_sent", app: "mail", risk: Risk::Read, audience: Audience::Private };
        match deep(&policies, &person("dana"), &tool) {
            Ok(Outcome::Allow { .. }) => {}
            other => panic!("a decision deep in the stack did not evaluate: {other:?}"),
        }
    }

    /// Task and schedule control follows the layered policy, with today's
    /// rule as the default: one's own, the owner's and admins' anything;
    /// and the organisation's policy can narrow it (#266).
    #[test]
    fn controls_follow_the_layered_policy_with_todays_rule_as_default() {
        use std::collections::BTreeMap;
        let person = |id: &str, roles: &[&str]| serde_json::from_value::<Person>(serde_json::json!({ "id": id, "groups": ["staff"], "roles": roles })).unwrap();
        let (carol, bob, alice, dave) = (person("carol", &[]), person("bob", &[]), person("alice", &["owner"]), person("dave", &["admin"]));
        let cancel = |id: &'static str, owner: &'static str| ControlFacts { op: "cancel_task", kind: "Task", id, owner };
        let policies = BTreeMap::new();
        let c = cancel("t1", "carol");
        assert!(control_allowed(&policies, &carol, &c).is_ok(), "own task");
        assert!(control_allowed(&policies, &alice, &c).is_ok(), "owner");
        assert!(control_allowed(&policies, &dave, &c).is_ok(), "admin");
        assert!(control_allowed(&policies, &bob, &c).is_err(), "another person's task");
        let narrow = format!("{DEFAULT_ORG}\nforbid(principal, action == Action::\"cancel_task\", resource) when {{ context.is_admin && !(resource in principal) }};");
        let mut narrowed = BTreeMap::new();
        narrowed.insert("org".to_string(), narrow);
        assert!(control_allowed(&narrowed, &dave, &c).is_err(), "admin cancel of another's allowed");
        let own = cancel("t2", "dave");
        assert!(control_allowed(&narrowed, &dave, &own).is_ok(), "admin cancel of own denied");
        assert!(control_allowed(&narrowed, &alice, &c).is_ok(), "owner cancel denied");
        let pause = ControlFacts { op: "pause_schedule", kind: "Schedule", id: "s1", owner: "carol" };
        assert!(control_allowed(&policies, &carol, &pause).is_ok(), "own schedule");
        assert!(control_allowed(&policies, &bob, &pause).is_err(), "another person's schedule");
        let mut broken = BTreeMap::new();
        broken.insert("org".to_string(), "permit(principal, action, resource) when {".to_string());
        assert!(control_allowed(&broken, &carol, &c).is_err(), "broken policy allowed");
    }

    /// A grant covers its person's actions in its apps, never moving money
    /// or anyone else's actions, and sends and irreversible actions only
    /// once lifted.
    #[test]
    fn a_grant_covers_its_apps_less_what_stays_excluded() {
        let carol = person("carol");
        let t = |name: &'static str, app: &'static str, risk: Risk, audience: Audience| ToolFacts { name, app, risk, audience };
        let save = t("crm__update_record", "crm", Risk::ReversibleWrite, Audience::Private);
        let post = t("team__post", "team", Risk::ReversibleWrite, Audience::Internal);
        let delete = t("crm__delete_all", "crm", Risk::Irreversible, Audience::Private);
        let pay = t("crm__pay", "crm", Risk::Financial, Audience::Private);
        let other = t("notes__save", "notes", Risk::ReversibleWrite, Audience::Private);
        let apps = vec!["crm".to_string(), "team".to_string()];
        let none = grant_cedar("carol", &apps, &[]).unwrap();
        assert!(grant_covers(&none, &carol, &save).unwrap());
        for x in [&post, &delete, &pay, &other] {
            assert!(!grant_covers(&none, &carol, x).unwrap(), "{} covered by default", x.name);
        }
        assert!(!grant_covers(&none, &person("bob"), &save).unwrap(), "someone else's action covered");
        let all = grant_cedar("carol", &apps, &["send".into(), "irreversible".into()]).unwrap();
        assert!(grant_covers(&all, &carol, &post).unwrap() && grant_covers(&all, &carol, &delete).unwrap());
        assert!(!grant_covers(&all, &carol, &pay).unwrap(), "moving money covered");
        assert!(grant_cedar("carol", &[], &[]).is_err(), "a grant over no app");
        // An app's name is quoted, never read as policy.
        let odd = grant_cedar("carol", &["crm\" }; permit(principal, action, resource); //".into()], &[]).unwrap();
        assert!(!grant_covers(&odd, &carol, &other).unwrap());
    }
}
