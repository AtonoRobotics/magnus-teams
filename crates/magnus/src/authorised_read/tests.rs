use super::*;
use crate::tools::{Audience, Label};
use serde_json::json;

fn ev(s: &mut State, seq: i64, actor: &str, e: Value) {
    s.apply(&crate::log::Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
}

/// Bob's own run.
fn world() -> State {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(&mut s, 1, "person:bob", json!({ "kind": "task_created", "task": "tb", "run": "rb", "person": "bob", "text": "read my mail", "config": config }));
    s
}

fn bob() -> Person {
    serde_json::from_value(json!({ "id": "bob", "groups": ["staff"] })).unwrap()
}

fn read(risk: Risk, connector: Value) -> Facts {
    let mut c = json!({ "name": "mail", "transport": "http", "url": "http://x", "audience": "private", "label": "private" });
    if let (Some(c), Some(extra)) = (c.as_object_mut(), connector.as_object()) {
        c.extend(extra.clone());
    }
    let c: crate::config::Connector = serde_json::from_value(c).unwrap();
    Facts {
        name: "mail__list".into(),
        app: "mail".into(),
        risk,
        label: Label::PRINCIPAL,
        audience: Audience::Private,
        connector: Some(c),
        remote: "list".into(),
        reaches_self: false,
    }
}

fn said(r: Result<(), Refused>) -> String {
    match r {
        Ok(()) => "goes".into(),
        Err(Refused::Held(h)) => format!("held: {}", h.reason()),
        Err(e) => format!("{e:?}"),
    }
}

/// Each refusal before anything goes out, for a read inside a run and a
/// poll alike: what is pulled or missing, and what the gate answers.
#[test]
fn the_gate_refuses_a_read_for_each_reason_before_it_goes_out() {
    type Setup = fn(&mut State);
    let none: Setup = |_| {};
    let cases: [(&str, Setup, Option<Facts>, &str); 9] = [
        ("an allowed read", none, Some(read(Risk::Read, json!({}))), "goes"),
        ("a tool the app does not class", none, None, "Unknown"),
        ("a write, not a read", none, Some(read(Risk::ReversibleWrite, json!({}))), "NotARead"),
        ("the server's switch", |s| ev(s, 2, "person:alice", json!({ "kind": "kill_switch", "engaged": true })), Some(read(Risk::Read, json!({}))), "held: kill_switch"),
        (
            "bob's switch",
            |s| ev(s, 2, "admin:root", json!({ "kind": "halt", "change": "engaged", "scope": { "level": "person", "id": "bob" } })),
            Some(read(Risk::Read, json!({}))),
            "held: kill_switch:person:bob",
        ),
        (
            "bob's agent's switch",
            |s| ev(s, 2, "admin:root", json!({ "kind": "halt", "change": "engaged", "scope": { "level": "agent", "id": "person:bob" } })),
            Some(read(Risk::Read, json!({}))),
            "held: kill_switch:agent:person:bob",
        ),
        (
            "the app's switch",
            |s| ev(s, 2, "admin:root", json!({ "kind": "halt", "change": "engaged", "scope": { "level": "connector", "id": "mail" } })),
            Some(read(Risk::Read, json!({}))),
            "held: kill_switch:connector:mail",
        ),
        ("bob turned off", |s| ev(s, 2, "kernel", json!({ "kind": "person_turned_off", "person": "bob", "why": "gone" })), Some(read(Risk::Read, json!({}))), "held: turned_off"),
        ("an app bob's groups may not use", none, Some(read(Risk::Read, json!({ "access_groups": ["finance"] }))), "NoRights"),
    ];
    for (what, setup, rf, want) in cases {
        let mut s = world();
        setup(&mut s);
        let run = s.runs["rb"].clone();
        assert_eq!(said(precheck(&s, &bob(), rf.as_ref(), Some(&run))), want, "{what}, inside a run");
        assert_eq!(said(precheck(&s, &bob(), rf.as_ref(), None)), want, "{what}, as a poll");
    }
}

/// A pause lets reads go on: what runs goes on, and a poll is no new work.
#[test]
fn a_paused_agent_still_reads() {
    let mut s = world();
    ev(&mut s, 2, "admin:root", json!({ "kind": "halt", "change": "paused", "agent": "person:bob" }));
    assert_eq!(said(precheck(&s, &bob(), Some(&read(Risk::Read, json!({}))), None)), "goes");
}

/// Policy judges the read under the run's task layer and taint; a poll
/// under the person's own layers.
#[test]
fn policy_judges_a_read_under_its_runs_task_and_a_poll_without_one() {
    let s = world();
    let run = s.runs["rb"].clone();
    let rf = read(Risk::Read, json!({}));
    let mut policies = BTreeMap::new();
    assert!(matches!(judge(&policies, &bob(), &rf, Some(&run)), Ok(Judged::Allow(_))));
    policies.insert("task/tb".to_string(), r#"forbid(principal, action, resource == Tool::"mail__list");"#.to_string());
    assert!(matches!(judge(&policies, &bob(), &rf, Some(&run)), Ok(Judged::Deny(..))), "the task's layer did not hold the read");
    assert!(matches!(judge(&policies, &bob(), &rf, None), Ok(Judged::Allow(_))), "another task's layer held a poll");
    policies.insert("user/bob".to_string(), r#"forbid(principal, action, resource == Tool::"mail__list");"#.to_string());
    assert!(matches!(judge(&policies, &bob(), &rf, None), Ok(Judged::Deny(..))), "bob's own layer did not hold his poll");
    policies.insert("org".to_string(), "permit(principal, action, resource) when {".to_string());
    assert!(matches!(judge(&policies, &bob(), &rf, None), Err(Refused::Unjudged(_))), "a broken policy judged");
}
