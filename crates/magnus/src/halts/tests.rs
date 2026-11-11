use super::*;
use serde_json::{json, Value};

fn ev(s: &mut State, seq: i64, actor: &str, e: Value) {
    s.apply(&crate::log::Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
}

fn created(s: &mut State, seq: i64, person: &str, task: &str, run: &str) {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(s, seq, &format!("person:{person}"), json!({ "kind": "task_created", "task": task, "run": run, "person": person, "text": "a secret plan", "config": config }));
}

/// Alice's own run, Bob's own run, and a team run Bob asked for.
fn world() -> State {
    let mut s = State::default();
    created(&mut s, 1, "alice", "ta", "ra");
    created(&mut s, 2, "bob", "tb", "rb");
    created(&mut s, 3, "bob", "tt", "rt");
    ev(&mut s, 4, "person:bob", json!({ "kind": "team", "change": "run", "workspace": "w", "channel": "c", "thread": null, "task": "tt", "requester": "bob" }));
    s
}

fn halt_of(s: &State, run: &str, tool: Option<&str>) -> Option<String> {
    halt(s, &s.runs[run], tool).map(|(reason, _)| reason)
}

fn engage(s: &mut State, seq: i64, scope: Value) {
    ev(s, seq, "admin:root", json!({ "kind": "halt", "change": "engaged", "scope": scope, "name": "Root" }));
}

#[test]
fn a_switch_is_its_own_event_kind_and_keeps_who_pulled_it_and_when() {
    let e: crate::events::Ev = serde_json::from_value(json!({ "kind": "halt", "change": "engaged", "scope": { "level": "connector", "id": "mail" }, "name": "Root" })).unwrap();
    let back = serde_json::to_value(&e).unwrap();
    assert_eq!(back["kind"], "halt");
    assert_eq!(back["scope"], json!({ "level": "connector", "id": "mail" }));
    let mut s = State::default();
    engage(&mut s, 1, json!({ "level": "person", "id": "alice" }));
    let p = &s.halts.on[&Scope::Person("alice".into())];
    assert_eq!((p.by.as_str(), p.name.as_str()), ("admin:root", "Root"));
    ev(&mut s, 2, "admin:root", json!({ "kind": "halt", "change": "released", "scope": { "level": "person", "id": "alice" } }));
    assert!(s.halts.on.is_empty(), "the release left the switch on");
}

#[test]
fn every_kill_reason_is_known_and_no_other() {
    assert!(is_kill("kill_switch"));
    assert!(is_kill(&Scope::Person("alice".into()).reason()));
    assert!(is_kill(&Scope::Connector("mail".into()).reason()));
    assert!(!is_kill("kill_switchy"));
    assert!(!is_kill("timer"));
}

#[test]
fn a_persons_switch_holds_everything_done_for_them_and_nothing_else() {
    let mut s = world();
    engage(&mut s, 5, json!({ "level": "person", "id": "bob" }));
    assert_eq!(halt_of(&s, "rb", None).as_deref(), Some("kill_switch:person:bob"));
    assert_eq!(halt_of(&s, "rt", None).as_deref(), Some("kill_switch:person:bob"), "the team agent's work Bob asked for went on");
    assert_eq!(halt_of(&s, "ra", None), None, "Alice's run was held by Bob's switch");
    assert!(holds_new(&s, "bob", false).is_some() && holds_new(&s, "bob", true).is_some());
    assert!(holds_new(&s, "alice", false).is_none() && holds_new(&s, "alice", true).is_none());
}

#[test]
fn an_agents_switch_holds_that_agent_alone() {
    let mut s = world();
    engage(&mut s, 5, json!({ "level": "agent", "id": "team" }));
    assert_eq!(halt_of(&s, "rt", None).as_deref(), Some("kill_switch:agent:team"));
    assert_eq!(halt_of(&s, "rb", None), None, "Bob's own agent was held by the team agent's switch");
    assert!(holds_new(&s, "bob", true).is_some() && holds_new(&s, "bob", false).is_none());
    engage(&mut s, 6, json!({ "level": "agent", "id": "person:alice" }));
    assert_eq!(halt_of(&s, "ra", None).as_deref(), Some("kill_switch:agent:person:alice"));
    assert!(holds_new(&s, "alice", false).is_some());
}

#[test]
fn a_connectors_switch_holds_a_call_to_it_and_takes_its_tools_away() {
    let mut s = world();
    engage(&mut s, 5, json!({ "level": "connector", "id": "mail" }));
    assert_eq!(halt_of(&s, "ra", None), None, "a run that does not call it was held");
    assert_eq!(halt_of(&s, "ra", Some("mail__send")).as_deref(), Some("kill_switch:connector:mail"));
    assert_eq!(halt_of(&s, "ra", Some("notes__list")), None);
    assert!(holds_new(&s, "alice", false).is_none(), "a connector's switch refused work that may not need it");
    let spec = |n: &str| magnus_adapter_proto::ToolSpec { name: n.into(), description: String::new(), input_schema: json!({}) };
    let kept: Vec<String> = offered(&s, vec![spec("mail__send"), spec("notes__list")]).into_iter().map(|t| t.name).collect();
    assert_eq!(kept, ["notes__list"]);
    // Running work that needs it: a run that has called it.
    ev(&mut s, 6, "kernel", json!({ "kind": "intent", "run": "ra", "call_id": "c1", "step_id": "s", "key": "k1", "tool": "mail__send", "input": {}, "issuer": "model" }));
    let scope = Scope::Connector("mail".into());
    assert!(covers(&s, &scope, &s.runs["ra"]), "a run that called it is not covered");
    assert!(!covers(&s, &scope, &s.runs["rb"]));
}

#[test]
fn the_servers_switch_holds_everything_first() {
    let mut s = world();
    ev(&mut s, 5, "person:alice", json!({ "kind": "kill_switch", "engaged": true }));
    for r in ["ra", "rb", "rt"] {
        assert_eq!(halt_of(&s, r, None).as_deref(), Some("kill_switch"));
    }
    assert!(due_schedules(&s, chrono::Utc::now()).is_empty());
}

#[test]
fn a_paused_agent_starts_nothing_new_and_its_running_work_goes_on() {
    let mut s = world();
    ev(&mut s, 5, "admin:root", json!({ "kind": "halt", "change": "paused", "agent": "person:alice", "name": "Root" }));
    assert_eq!(halt_of(&s, "ra", None), None, "a pause stopped running work");
    assert!(holds_new(&s, "alice", false).unwrap().contains("paused"));
    assert!(holds_new(&s, "alice", true).is_none(), "pausing Alice's agent paused the team agent's work for her");
    ev(&mut s, 6, "admin:root", json!({ "kind": "halt", "change": "unpaused", "agent": "person:alice" }));
    assert!(holds_new(&s, "alice", false).is_none());
}

#[test]
fn a_held_persons_schedules_do_not_fire() {
    let mut s = world();
    ev(&mut s, 5, "person:alice", json!({ "kind": "schedule_set", "id": "sa", "person": "alice", "every_minutes": 1 }));
    ev(&mut s, 6, "person:bob", json!({ "kind": "schedule_set", "id": "sb", "person": "bob", "every_minutes": 1 }));
    let later = chrono::Utc::now() + chrono::Duration::hours(1);
    assert_eq!(due_schedules(&s, later).len(), 2);
    engage(&mut s, 7, json!({ "level": "person", "id": "bob" }));
    let due: Vec<String> = due_schedules(&s, later).into_iter().map(|x| x.id).collect();
    assert_eq!(due, ["sa"]);
}

#[test]
fn the_agents_list_names_who_asked_and_what_was_spent_and_no_ones_words() {
    let s = world();
    let v = agents::agents(&s, &[("alice".into(), "Alice".into()), ("bob".into(), "Bob".into())]);
    let ids: Vec<&str> = v["agents"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["team", "person:alice", "person:bob"]);
    let team = &v["agents"][0];
    assert_eq!(team["running"][0]["run"], "rt");
    assert_eq!(team["running"][0]["asked_by"]["person"], "bob");
    let bob = &v["agents"][2];
    assert_eq!(bob["running"].as_array().unwrap().len(), 1, "the team run was counted as Bob's own agent's: {bob}");
    assert!(bob["spent_usd"].is_number());
    assert!(!v.to_string().contains("a secret plan"), "the list showed a task's words");
}

#[test]
fn the_banner_reaches_the_people_a_switch_affects_and_no_one_else() {
    let mut s = world();
    ev(&mut s, 5, "person:carol", json!({ "kind": "team", "change": "created", "workspace": "w", "name": "W", "owner": "carol" }));
    ev(&mut s, 5, "person:carol", json!({ "kind": "team", "change": "member", "workspace": "w", "person": "bob", "role": "member" }));
    engage(&mut s, 6, json!({ "level": "person", "id": "alice" }));
    engage(&mut s, 7, json!({ "level": "agent", "id": "team" }));
    engage(&mut s, 8, json!({ "level": "connector", "id": "mail" }));
    let levels = |p: &str| agents::stops_for(&s, p).into_iter().map(|v| format!("{}:{}", v["level"].as_str().unwrap(), v["target"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(levels("alice"), ["person:alice", "connector:mail"]);
    assert_eq!(levels("bob"), ["agent:team", "connector:mail"], "a member did not see the team agent's switch");
    let one = &agents::stops_for(&s, "alice")[0];
    assert_eq!((one["by"].as_str(), one["by_name"].as_str()), (Some("admin:root"), Some("Root")));
    assert!(one["at"].is_string(), "the banner does not say when: {one}");
}

#[test]
fn scopes_parse_only_as_agents_exist() {
    assert_eq!(Scope::parse("agent", "team"), Some(Scope::Agent("team".into())));
    assert_eq!(Scope::parse("agent", "person:bob"), Some(Scope::Agent("person:bob".into())));
    assert_eq!(Scope::parse("agent", "bob"), None);
    assert_eq!(Scope::parse("agent", "person:"), None);
    assert_eq!(Scope::parse("person", " "), None);
    assert_eq!(Scope::parse("server", "x"), None);
}
