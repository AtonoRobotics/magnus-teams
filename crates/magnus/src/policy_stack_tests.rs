//! The layer stack's answers, pinned: what `decide`, `control_allowed` and
//! `loop_allowed` say, outcome and words, across a broad table of layer
//! stacks. The three share one hierarchy rule (only `org` widens); this
//! table holds each one's own small differences in place while that rule
//! has one home. The expected answers are in `policy_stack_golden.txt`,
//! one line a case; a mismatch prints every line as it now stands.

use super::*;
use crate::accounts::Role as Directory;

const SEND: &str = r#"forbid(principal, action, resource == Tool::"mail__send");"#;
const SEND_EXEC: &str = r#"forbid(principal, action == Action::"execute", resource == Tool::"mail__send");"#;
const UNTRUSTED: &str = r#"forbid(principal, action, resource) when { context.untrusted };"#;
const EVERYTHING: &str = "forbid(principal, action, resource);";
const PERMIT_ALL: &str = "permit(principal, action, resource);";
const ASK_ONLY: &str = r#"permit(principal, action == Action::"request_approval", resource);"#;
const CANCEL: &str = r#"forbid(principal, action == Action::"cancel_task", resource);"#;
const CANCEL_ADMIN: &str = r#"forbid(principal, action == Action::"cancel_task", resource) when { context.is_admin && !(resource in principal) };"#;
const SUGGEST: &str = r#"forbid(principal, action == Action::"suggest", resource);"#;
const ACT_MAIL: &str = r#"forbid(principal, action == Action::"act", resource in App::"mail");"#;
const MIXED: &str = r#"forbid(principal, action in [Action::"execute", Action::"act"], resource in App::"mail");"#;
const BROKEN: &str = "permit(principal, action, resource) when {";

/// The layer stacks: a name, and each layer's text.
fn stacks() -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    let s = |t: &str| t.to_string();
    let org_plus = |t: &str| format!("{DEFAULT_ORG}\n{t}");
    vec![
        ("defaults", vec![]),
        ("org empty", vec![("org", s(""))]),
        ("org permit all", vec![("org", s(PERMIT_ALL))]),
        ("org ask only", vec![("org", s(ASK_ONLY))]),
        ("org forbids send", vec![("org", org_plus(SEND))]),
        ("org forbids send execute", vec![("org", org_plus(SEND_EXEC))]),
        ("org forbids untrusted", vec![("org", org_plus(UNTRUSTED))]),
        ("org forbids everything", vec![("org", s(EVERYTHING))]),
        ("org forbids cancel", vec![("org", org_plus(CANCEL))]),
        ("org forbids admin cancel", vec![("org", org_plus(CANCEL_ADMIN))]),
        ("org forbids suggest", vec![("org", org_plus(SUGGEST))]),
        ("org forbids act mail", vec![("org", org_plus(ACT_MAIL))]),
        ("org mixed", vec![("org", org_plus(MIXED))]),
        ("staff forbids send", vec![("group/staff", s(SEND))]),
        ("staff permits all", vec![("group/staff", s(PERMIT_ALL))]),
        ("staff forbids everything", vec![("group/staff", s(EVERYTHING))]),
        ("staff forbids cancel", vec![("group/staff", s(CANCEL))]),
        ("staff forbids suggest", vec![("group/staff", s(SUGGEST))]),
        ("staff mixed", vec![("group/staff", s(MIXED))]),
        ("other group forbids everything", vec![("group/ops", s(EVERYTHING))]),
        ("user forbids send", vec![("user/carol", s(SEND))]),
        ("user forbids untrusted", vec![("user/carol", s(UNTRUSTED))]),
        ("user forbids cancel", vec![("user/carol", s(CANCEL))]),
        ("user forbids act mail", vec![("user/carol", s(ACT_MAIL))]),
        ("another user forbids everything", vec![("user/bob", s(EVERYTHING))]),
        ("task forbids send", vec![("task/t1", s(SEND))]),
        ("task forbids everything", vec![("task/t1", s(EVERYTHING))]),
        ("staff and user forbid send", vec![("group/staff", s(SEND)), ("user/carol", s(SEND))]),
        ("finance and staff forbid", vec![("group/finance", s(EVERYTHING)), ("group/staff", s(SEND))]),
        ("staff cancel and user suggest", vec![("group/staff", s(CANCEL)), ("user/carol", s(SUGGEST))]),
        ("org send and user everything", vec![("org", org_plus(SEND)), ("user/carol", s(EVERYTHING))]),
        ("org ask only and staff send", vec![("org", s(ASK_ONLY)), ("group/staff", s(SEND))]),
        ("org empty and task everything", vec![("org", s("")), ("task/t1", s(EVERYTHING))]),
        ("every layer forbids", vec![("org", org_plus(SEND)), ("group/staff", s(SEND)), ("group/finance", s(SEND)), ("user/carol", s(SEND)), ("task/t1", s(SEND))]),
        ("org broken", vec![("org", s(BROKEN))]),
        ("staff broken", vec![("group/staff", s(BROKEN))]),
        ("user broken", vec![("user/carol", s(BROKEN))]),
    ]
}

fn person(id: &str, roles: &[&str]) -> Person {
    serde_json::from_value(serde_json::json!({ "id": id, "groups": ["staff", "finance"], "roles": roles })).unwrap()
}

fn tools() -> Vec<ToolFacts<'static>> {
    vec![
        ToolFacts { name: "mail__list", app: "mail", risk: Risk::Read, audience: Audience::Private },
        ToolFacts { name: "mail__send", app: "mail", risk: Risk::Irreversible, audience: Audience::Public },
        ToolFacts { name: "crm__pay", app: "crm", risk: Risk::Financial, audience: Audience::Private },
    ]
}

fn decided(r: Result<Outcome>) -> String {
    match r {
        Ok(o) => format!("{o:?}"),
        Err(e) => format!("Err({e:#})"),
    }
}

fn controlled(r: Result<String, ControlDeny>) -> String {
    match r {
        Ok(rule) => format!("Ok({rule})"),
        Err(ControlDeny::Forbidden(m)) => format!("Forbidden({m})"),
        Err(ControlDeny::Broken(m)) => format!("Broken({m})"),
    }
}

fn looped(r: Result<String, LoopDeny>) -> String {
    match r {
        Ok(rule) => format!("Ok({rule})"),
        Err(LoopDeny::Forbidden(m)) => format!("Forbidden({m})"),
        Err(LoopDeny::Broken(m)) => format!("Broken({m})"),
    }
}

/// Every answer, one line each.
fn answers() -> Vec<String> {
    let carol = person("carol", &[]);
    let dave = person("dave", &["admin"]);
    let alice = person("alice", &["owner"]);
    let mut out = vec![];
    for (name, layers) in stacks() {
        let policies: BTreeMap<String, String> = layers.into_iter().map(|(l, t)| (l.to_string(), t)).collect();
        for tool in tools() {
            for (untrusted, private) in [(false, false), (true, false), (false, true)] {
                for task in [None, Some("t1")] {
                    let r = decided(decide(&policies, &carol, task, &tool, untrusted, private));
                    out.push(format!("{name} | decide {} u={untrusted} p={private} task={task:?} | {r}", tool.name));
                }
            }
            let r = looped(loop_allowed(&policies, &carol, "act", Some(&tool), false, false));
            out.push(format!("{name} | loop act {} | {r}", tool.name));
        }
        for op in ["suggest", "interrupt"] {
            for untrusted in [false, true] {
                let r = looped(loop_allowed(&policies, &carol, op, None, untrusted, false));
                out.push(format!("{name} | loop {op} u={untrusted} | {r}"));
            }
        }
        for who in [&carol, &dave, &alice] {
            for (op, kind, id, owner) in [
                ("cancel_task", "Task", "t1", "carol"),
                ("cancel_task", "Task", "t2", "bob"),
                ("pause_schedule", "Schedule", "s1", "carol"),
                ("edit_schedule", "Schedule", "s2", "bob"),
            ] {
                let facts = ControlFacts { op, kind, id, owner };
                let r = controlled(control_allowed(&policies, who, Authority::of(who, Directory::Member), &facts));
                out.push(format!("{name} | control {} {op} {kind}/{id} of {owner} | {r}", who.id));
            }
        }
    }
    out
}

#[test]
fn the_layer_stack_answers_as_it_always_has() {
    let now = answers();
    let golden: Vec<&str> = include_str!("policy_stack_golden.txt").lines().collect();
    if now.len() != golden.len() || now.iter().zip(&golden).any(|(a, b)| a != b) {
        let first = now.iter().zip(&golden).position(|(a, b)| a != b);
        println!("----- golden begins -----\n{}\n----- golden ends -----", now.join("\n"));
        panic!("the stack's answers changed ({} now, {} pinned); first difference at line {first:?}", now.len(), golden.len());
    }
}
