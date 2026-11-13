//! The agent policy as an admin edits it (admin console spec, Security ›
//! Agent policy): each Cedar policy set (the organisation's, a group's, a
//! person's), a draft of each beside what is live, validation against
//! Magnus's schema with each error's place in the text, a test of any
//! request against the draft or the live policy showing the decision and
//! the rules that decided it, Apply, and every version kept to roll back
//! to.

use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::{Audience, Risk};
use axum::http::StatusCode;
use cedar_policy::{ActionConstraint, Effect, PolicySet, Schema, ValidationMode, Validator};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

pub type Refusal = (StatusCode, String);

/// What a policy may name: people in groups, tools in apps with their risk
/// and audience, the two actions, and the run's taint as context; and tasks
/// and schedules in their person, the five control actions, and whether the
/// controller is the owner or an admin (#266).
pub const SCHEMA: &str = r#"
entity Group;
entity Person in [Group];
entity App;
entity Tool in [App] = { "risk": String, "audience": String };
entity Task in [Person];
entity Schedule in [Person];
action "execute", "request_approval" appliesTo {
  principal: [Person],
  resource: [Tool],
  context: { "untrusted": Bool, "private": Bool }
};
action "cancel_task", "pause_schedule", "resume_schedule", "edit_schedule", "cancel_schedule" appliesTo {
  principal: [Person],
  resource: [Task, Schedule],
  context: { "is_owner": Bool, "is_admin": Bool }
};
action "suggest", "interrupt" appliesTo {
  principal: [Person],
  resource: [Person],
  context: { "untrusted": Bool, "private": Bool }
};
action "act" appliesTo {
  principal: [Person],
  resource: [Tool],
  context: { "untrusted": Bool, "private": Bool }
};
"#;

/// The schema's tool half: people in groups, tools in apps, the two
/// actions. Validation runs each side's rules against its half (`check`),
/// so an unqualified tool rule is judged as tools, not against tasks.
const TOOL_SCHEMA: &str = r#"
entity Group;
entity Person in [Group];
entity App;
entity Tool in [App] = { "risk": String, "audience": String };
action "execute", "request_approval" appliesTo {
  principal: [Person],
  resource: [Tool],
  context: { "untrusted": Bool, "private": Bool }
};
"#;

/// The schema's control half: tasks and schedules in their person, the
/// five control actions.
const CONTROL_SCHEMA: &str = r#"
entity Group;
entity Person in [Group];
entity Task in [Person];
entity Schedule in [Person];
action "cancel_task", "pause_schedule", "resume_schedule", "edit_schedule", "cancel_schedule" appliesTo {
  principal: [Person],
  resource: [Task, Schedule],
  context: { "is_owner": Bool, "is_admin": Bool }
};
"#;

/// A schema half, parsed once. Each is compiled in and parsed by a test; were one broken, every
/// policy would be refused with the reason rather than the server stop.
type Parsed = std::sync::OnceLock<Result<Schema, String>>;

fn parsed(cell: &'static Parsed, src: &str) -> Result<&'static Schema, String> {
    cell.get_or_init(|| Schema::from_cedarschema_str(src).map(|s| s.0).map_err(|e| format!("Magnus's policy schema does not parse: {e}"))).as_ref().map_err(Clone::clone)
}

fn tool_schema() -> Result<&'static Schema, String> {
    static S: Parsed = std::sync::OnceLock::new();
    parsed(&S, TOOL_SCHEMA)
}

fn control_schema() -> Result<&'static Schema, String> {
    static S: Parsed = std::sync::OnceLock::new();
    parsed(&S, CONTROL_SCHEMA)
}

/// The schema's loop half (#368): suggest and interrupt spend a
/// person's attention, so their resource is the person; acts touch
/// apps, so theirs is the tool, as for execute.
const LOOP_SCHEMA: &str = r#"
entity Group;
entity Person in [Group];
entity App;
entity Tool in [App] = { "risk": String, "audience": String };
action "suggest", "interrupt" appliesTo {
  principal: [Person],
  resource: [Person],
  context: { "untrusted": Bool, "private": Bool }
};
action "act" appliesTo {
  principal: [Person],
  resource: [Tool],
  context: { "untrusted": Bool, "private": Bool }
};
"#;

fn loop_schema() -> Result<&'static Schema, String> {
    static S: Parsed = std::sync::OnceLock::new();
    parsed(&S, LOOP_SCHEMA)
}

fn served_schema() -> Result<&'static Schema, String> {
    static S: Parsed = std::sync::OnceLock::new();
    parsed(&S, SCHEMA)
}

/// Where an offset falls in the text: its line and column, from 1.
fn place(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

fn problem(text: &str, d: &dyn miette::Diagnostic, message: String) -> Value {
    let span = d.labels().and_then(|mut l| l.next()).map(|l| (l.offset(), l.len()));
    let (line, column) = span.map(|(o, _)| place(text, o)).unwrap_or((1, 1));
    json!({ "message": message, "line": line, "column": column, "offset": span.map(|s| s.0), "length": span.map(|s| s.1) })
}

/// Parse a policy set and check it against the schema: each error with its
/// place. Empty: it may be applied. Each rule is judged against its own
/// side's half — the text is validated whole against each half and the
/// other side's errors dropped by policy id, so places stay true — and an
/// unqualified forbid that would also fit control is refused with guidance
/// instead of silently constraining tool calls only (#266).
pub fn check(text: &str) -> Vec<Value> {
    let set = match PolicySet::from_str(text) {
        Ok(s) => s,
        Err(e) => return e.iter().map(|x| problem(text, x, x.to_string())).collect(),
    };
    let (tool_schema, control_schema, loop_schema, served_schema) = match (tool_schema(), control_schema(), loop_schema(), served_schema()) {
        (Ok(t), Ok(c), Ok(l), Ok(s)) => (t, c, l, s),
        (t, c, l, s) => {
            return [t.err(), c.err(), l.err(), s.err()].into_iter().flatten().map(|m| json!({ "message": m, "line": 1, "column": 1, "offset": null, "length": null })).collect()
        }
    };
    let mut tools = HashSet::new();
    let mut controls = HashSet::new();
    let mut loops = HashSet::new();
    let mut mixed = HashSet::new();
    let mut unqualified = vec![];
    for p in set.policies() {
        let id = p.id().to_string();
        let (t, c, l) = crate::policy::policy_sides(&p);
        // A rule bearing on several sides validates against the whole
        // schema: no half declares every action it names (#403).
        if [t, c, l].iter().filter(|&&x| x).count() > 1 {
            mixed.insert(id.clone());
        } else {
            if t {
                tools.insert(id.clone());
            }
            if c {
                controls.insert(id.clone());
            }
            if l {
                loops.insert(id.clone());
            }
        }
        if matches!(p.action_constraint(), ActionConstraint::Any) {
            unqualified.push((id, p.effect() == Effect::Forbid));
        }
    }
    let mut out = vec![];
    let mut tool_bad = HashSet::new();
    for e in Validator::new(tool_schema.clone()).validate(&set, ValidationMode::Strict).validation_errors() {
        let id = e.policy_id().to_string();
        tool_bad.insert(id.clone());
        if tools.contains(id.as_str()) {
            out.push(problem(text, e, e.to_string()));
        }
    }
    let mut control_bad = HashSet::new();
    for e in Validator::new(control_schema.clone()).validate(&set, ValidationMode::Strict).validation_errors() {
        let id = e.policy_id().to_string();
        control_bad.insert(id.clone());
        if controls.contains(id.as_str()) {
            out.push(problem(text, e, e.to_string()));
        }
    }
    let mut loop_bad = HashSet::new();
    for e in Validator::new(loop_schema.clone()).validate(&set, ValidationMode::Strict).validation_errors() {
        let id = e.policy_id().to_string();
        loop_bad.insert(id.clone());
        if loops.contains(id.as_str()) {
            out.push(problem(text, e, e.to_string()));
        }
    }
    for e in Validator::new(served_schema.clone()).validate(&set, ValidationMode::Strict).validation_errors() {
        if mixed.contains(e.policy_id().to_string().as_str()) {
            out.push(problem(text, e, e.to_string()));
        }
    }
    for (id, is_forbid) in unqualified {
        if is_forbid {
            // One message naming every other side the forbid would fit
            // if it named actions. Control keeps its existing signal
            // (fits the control half); the loop guides only when the
            // rule fails as tools but fits as loop — a Tool-scoped
            // rule already works as tools, so loop guidance there
            // would refuse legitimate policies (#368).
            let mut sides = vec![];
            if !control_bad.contains(&id) {
                sides.push("name the task and schedule actions to constrain control");
            }
            if !loop_bad.contains(&id) && tool_bad.contains(&id) {
                sides.push("name suggest, interrupt or act to constrain the loop");
            }
            if !sides.is_empty() {
                out.push(json!({ "message": format!("for policy `{id}`, a forbid that names no action constrains tool calls only; {}", sides.join("; ")), "line": 1, "column": 1, "offset": Value::Null, "length": Value::Null }));
            }
        }
    }
    out
}

/// A layer an admin may read and write: the organisation's or a group's. A
/// person's own layer and their tasks' are theirs alone (the owner,
/// 2026-11-11), so the admin's editor neither lists nor changes them.
pub fn layer_ok(_k: &Kernel, layer: &str) -> bool {
    crate::policy_layers::kept_by_the_server(layer) && layer.len() <= 106
}

impl Kernel {
    /// Every policy set: what is live (the organisation's default when none
    /// was written), the draft if there is one, and its versions.
    pub fn policy_sets(&self) -> Vec<Value> {
        let (live, drafts, history) = self.read(|s| (s.policies.clone(), s.policy_drafts.clone(), s.policy_history.clone()));
        let mut layers: Vec<String> = live.keys().chain(drafts.keys()).filter(|l| crate::policy_layers::kept_by_the_server(l)).cloned().collect();
        layers.push("org".into());
        layers.sort();
        layers.dedup();
        layers
            .into_iter()
            .map(|l| {
                let versions: Vec<&crate::state::PolicyVersion> = history.iter().filter(|v| v.layer == l).collect();
                let last = versions.last();
                json!({
                    "id": l,
                    "live": live.get(&l).cloned().or_else(|| (l == "org").then(|| crate::policy::DEFAULT_ORG.trim_start().to_string())),
                    "default": l == "org" && !live.contains_key("org"),
                    "draft": drafts.get(&l),
                    "versions": versions.len(),
                    "changed_at": last.map(|v| v.at), "changed_by": last.map(|v| v.by.clone()),
                })
            })
            .collect()
    }

    /// Keep a draft of a layer (None: drop it). Nothing is in force until
    /// it is applied.
    pub async fn policy_draft(&self, actor: &str, layer: &str, cedar: Option<String>) -> Result<Value, Refusal> {
        if !layer_ok(self, layer) {
            return Err((StatusCode::NOT_FOUND, "no such policy layer: org, group/<name>, user/<person> or task/<task>".into()));
        }
        if cedar.as_ref().is_some_and(|c| c.len() > 256 * 1024) {
            return Err((StatusCode::BAD_REQUEST, "a policy set is at most 256 KB".into()));
        }
        let mut problems = cedar.as_deref().map(check).unwrap_or_default();
        if let Some(Err(why)) = cedar.as_deref().map(|c| crate::policy_layers::tightens_only(layer, c)) {
            problems.push(json!({ "message": why }));
        }
        self.emit(actor, Ev::PolicyDrafted { layer: layer.into(), cedar }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        Ok(json!({ "id": layer, "problems": problems }))
    }

    /// Put a layer's policy in force, as `actor`: only one that parses and
    /// fits the schema. Its draft goes.
    pub async fn policy_apply(&self, actor: &str, layer: &str, cedar: &str) -> Result<Value, Refusal> {
        if !layer_ok(self, layer) {
            return Err((StatusCode::NOT_FOUND, "no such policy layer".into()));
        }
        let mut problems = check(cedar);
        if let Err(why) = crate::policy_layers::tightens_only(layer, cedar) {
            problems.push(json!({ "message": why }));
        }
        if !problems.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("the policy has {} problem{}: {}", problems.len(), if problems.len() == 1 { "" } else { "s" }, problems[0]["message"].as_str().unwrap_or_default()),
            ));
        }
        let had_draft = self.read(|s| s.policy_drafts.contains_key(layer));
        let mut evs = vec![(actor, Ev::PolicySet { layer: layer.into(), cedar: cedar.into() })];
        if had_draft {
            evs.push((actor, Ev::PolicyDrafted { layer: layer.into(), cedar: None }));
        }
        let done = self.emit_all(evs).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        Ok(json!({ "id": layer, "version": self.read(|s| s.policy_history.iter().filter(|v| v.layer == layer).count()), "seq": done.first().map(|e| e.seq) }))
    }

    /// Every version of a layer, newest first.
    pub fn policy_history(&self, layer: &str) -> Vec<Value> {
        if !layer_ok(self, layer) {
            return vec![];
        }
        let mut v: Vec<Value> = self.read(|s| {
            s.policy_history
                .iter()
                .filter(|v| v.layer == layer)
                .enumerate()
                .map(|(i, v)| json!({ "version": i + 1, "seq": v.seq, "at": v.at, "by": v.by, "cedar": v.cedar }))
                .collect()
        });
        v.reverse();
        v
    }

    /// A request tried against the policy as it is, or with drafts in place
    /// of what is live: the decision, and the rules that decided it.
    pub fn policy_test(&self, b: &Value) -> Result<Value, Refusal> {
        let s = |f: &str| b[f].as_str().unwrap_or_default().trim().to_string();
        let principal = s("principal");
        let Some(person): Option<Person> = self.person(&principal) else { return Err((StatusCode::BAD_REQUEST, format!("no person {principal:?} on this appliance"))) };
        let tool = s("resource");
        if tool.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "name the tool (the resource)".into()));
        }
        let known = crate::boundary::facts(self, &tool);
        let risk = Risk::parse(&s("risk"))
            .or(known.as_ref().map(|f| f.risk))
            .ok_or((StatusCode::BAD_REQUEST, "give the tool's risk: read, sandbox, reversible_write, irreversible, financial or prohibited".to_string()))?;
        let audience = Audience::parse(&s("audience")).or(known.as_ref().map(|f| f.audience)).unwrap_or(Audience::Private);
        let app = tool.split_once("__").map(|(a, _)| a.to_string()).unwrap_or_else(|| tool.clone());
        let facts = crate::policy::ToolFacts { name: &tool, app: &app, risk, audience };
        // Only the layers the server keeps: a person's own rules and their
        // tasks' are theirs, and may narrow what this answer says.
        let mut policies: BTreeMap<String, String> = self.read(|s| s.policies.clone());
        policies.retain(|l, _| crate::policy_layers::kept_by_the_server(l));
        if b["drafts"].as_bool().unwrap_or(false) {
            for (l, d) in self.read(|s| s.policy_drafts.clone()) {
                if crate::policy_layers::kept_by_the_server(&l) {
                    policies.insert(l, d);
                }
            }
        }
        if let Some(over) = b["cedar"].as_object() {
            for (l, c) in over {
                if let Some(c) = c.as_str().filter(|_| crate::policy_layers::kept_by_the_server(l)) {
                    policies.insert(l.clone(), c.to_string());
                }
            }
        }
        let layers: Vec<String> = crate::policy::layers_for(&policies, &person, None).into_iter().map(|(l, _)| l).collect();
        let outcome = crate::policy::decide(&policies, &person, None, &facts, b["untrusted"].as_bool().unwrap_or(false), b["private"].as_bool().unwrap_or(false))
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("{e:#}")))?;
        let (decision, rule, reason) = match outcome {
            crate::policy::Outcome::Allow { rule } => ("allow", Some(rule), None),
            crate::policy::Outcome::Ask { rule } => ("ask", Some(rule), None),
            crate::policy::Outcome::Deny { reason, rule } => ("deny", rule, Some(reason)),
        };
        Ok(json!({ "decision": decision, "rule": rule, "reason": reason, "layers": layers, "risk": risk.as_str(), "audience": audience.as_str() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_parses_and_the_default_policy_fits_it() {
        for half in [tool_schema(), control_schema(), loop_schema(), served_schema()] {
            half.unwrap();
        }
        assert!(check(crate::policy::DEFAULT_ORG).is_empty(), "{:?}", check(crate::policy::DEFAULT_ORG));
    }

    /// A control narrowing names control actions and task resources, so the
    /// organisation can write it (#266).
    #[test]
    fn a_control_narrowing_fits_the_schema() {
        let narrowing = r#"forbid(principal, action == Action::"cancel_task", resource) when { context.is_admin && !(resource in principal) };"#;
        assert!(check(narrowing).is_empty(), "{:?}", check(narrowing));
    }

    /// A rule naming actions from two sides validates against the whole
    /// schema, so one rule can constrain ordinary and autonomous calls
    /// together; neither half-schema alone declares both actions (#403).
    #[test]
    fn a_mixed_tool_and_loop_rule_validates() {
        let mixed = r#"forbid(principal, action in [Action::"execute", Action::"act"], resource in App::"mail");"#;
        assert!(check(mixed).is_empty(), "{:?}", check(mixed));
    }

    /// An unqualified tool rule validates as a tool rule, so the group's
    /// sales policy still drafts clean; a forbid that names no action but
    /// could constrain control is refused with guidance instead of
    /// silently constraining tool calls only (#266).
    #[test]
    fn an_unqualified_rule_validates_as_a_tool_rule() {
        let sales = "forbid(principal, action, resource) when { resource.risk == \"financial\" };";
        assert!(check(sales).is_empty(), "{:?}", check(sales));
        let blanket = "forbid(principal, action, resource);";
        let p = check(blanket);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0]["message"].as_str().unwrap_or("").contains("names no action"), "{p:?}");
        let preamble = "permit(principal, action, resource);";
        assert!(check(preamble).is_empty(), "{:?}", check(preamble));
    }

    #[test]
    fn a_problem_names_its_line_and_column() {
        let p = check("permit(principal, action, resource);\nforbid(principal, action == Action::\"execute\", resource) when { resource.colour == \"red\" };");
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0]["line"], 2, "{p:?}");
        let p = check("forbid(principal, action, resource) when { resource.colour == \"red\" };");
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0]["message"].as_str().unwrap_or("").contains("`Tool`"), "{p:?}");
        let p = check("permit(principal, action, resource)\n  when { context.untrusted && ;");
        assert!(!p.is_empty() && p[0]["line"] == 2, "{p:?}");
        assert!(check("permit(principal, action == Action::\"execute\", resource) when { context.private == false };").is_empty());
    }
}
