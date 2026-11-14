//! The bridge's server half: consent in the log, where each program stands,
//! routing a call to the right computer, and the policy before it.
use super::*;
use crate::log::Event;
use crate::policy::decide;
use crate::state::State;
use std::collections::BTreeMap;

fn person(id: &str, groups: &[&str]) -> Person {
    serde_json::from_value(json!({ "id": id, "groups": groups })).unwrap()
}

fn apply(s: &mut State, seq: i64, c: Change) {
    // Through the wire form, as a replay reads it.
    let ev: crate::events::Ev = serde_json::from_value(serde_json::to_value(crate::events::Ev::Desktop(c)).unwrap()).unwrap();
    s.apply(&Event { seq, at: chrono::Utc::now(), actor: "person:x".into(), ev });
}

fn reported(program: &str, running: bool, tools: &[&str]) -> Reported {
    let tools = tools
        .iter()
        .map(|t| ReportedTool { name: t.to_string(), description: format!("does {t}"), input_schema: json!({ "type": "object", "properties": { "code": { "type": "string" } } }) })
        .collect();
    Reported { program: program.into(), version: Some("4.2".into()), path: Some("/usr/bin/x".into()), running, tools }
}

fn device(id: &str, name: &str, allowed: &[&str]) -> Device {
    Device { id: id.into(), person: "mo".into(), name: name.into(), allowed: allowed.iter().map(|s| s.to_string()).collect() }
}

/// The kind's wire form is fixed: a kind is never edited.
#[test]
fn a_consent_reads_back_from_its_fixed_wire_form() {
    let ev: crate::events::Ev =
        serde_json::from_value(json!({ "kind": "desktop", "change": "consent", "device": "d1", "person": "mo", "program": "blender", "allowed": true })).unwrap();
    assert!(matches!(ev, crate::events::Ev::Desktop(Change::Consent { allowed: true, .. })));
    let ev: crate::events::Ev = serde_json::from_value(json!({ "kind": "desktop", "change": "device", "id": "d1", "person": "mo", "name": "Mo's laptop" })).unwrap();
    assert!(matches!(ev, crate::events::Ev::Desktop(Change::Device { .. })));
}

/// One consent per program per computer, kept by the log and replayed;
/// taken back, and gone with the computer. Another person's event changes
/// nothing on Mo's computer.
#[test]
fn consent_is_per_program_per_computer_and_replays() {
    let mut s = State::default();
    apply(&mut s, 1, Change::Device { id: "laptop01".into(), person: "mo".into(), name: "Mo's laptop".into() });
    apply(&mut s, 2, Change::Device { id: "desk0001".into(), person: "mo".into(), name: "Mo's desk".into() });
    apply(&mut s, 3, Change::Consent { device: "desk0001".into(), person: "mo".into(), program: "blender".into(), allowed: true });
    apply(&mut s, 4, Change::Consent { device: "desk0001".into(), person: "ana".into(), program: "freecad".into(), allowed: true });
    apply(&mut s, 5, Change::Device { id: "desk0001".into(), person: "ana".into(), name: "Ana's now".into() });
    let mine = s.local.desktop.of("mo");
    assert_eq!(mine.len(), 2);
    let desk = s.local.desktop.get("desk0001").unwrap();
    assert_eq!((desk.name.as_str(), desk.allowed.iter().cloned().collect::<Vec<_>>()), ("Mo's desk", vec!["blender".to_string()]), "another person's event changed Mo's computer");
    assert!(s.local.desktop.get("laptop01").unwrap().allowed.is_empty(), "a consent on one computer held on another");
    assert!(s.local.desktop.of("ana").is_empty());

    apply(&mut s, 6, Change::Consent { device: "desk0001".into(), person: "mo".into(), program: "blender".into(), allowed: false });
    assert!(s.local.desktop.get("desk0001").unwrap().allowed.is_empty(), "taking it back did not hold");
    apply(&mut s, 7, Change::Consent { device: "desk0001".into(), person: "mo".into(), program: "blender".into(), allowed: true });
    apply(&mut s, 8, Change::Removed { id: "desk0001".into(), person: "mo".into() });
    apply(&mut s, 9, Change::Device { id: "desk0001".into(), person: "mo".into(), name: "Mo's desk".into() });
    assert!(s.local.desktop.get("desk0001").unwrap().allowed.is_empty(), "a consent outlived its computer");
}

#[test]
fn each_program_is_not_installed_not_enabled_closed_or_ready() {
    let r = reported("blender", true, &["run_python"]);
    let closed = reported("blender", false, &[]);
    assert_eq!(ProgramState::of(true, true, None), ProgramState::NotInstalled);
    assert_eq!(ProgramState::of(false, true, Some(&r)), ProgramState::NotEnabled);
    assert_eq!(ProgramState::of(true, true, Some(&closed)), ProgramState::Closed);
    assert_eq!(ProgramState::of(true, false, Some(&r)), ProgramState::Closed, "a computer not connected was ready");
    assert_eq!(ProgramState::of(true, true, Some(&r)), ProgramState::Ready);
    let v = view(&device("desk0001", "Mo's desk", &["blender"]), true, &[r.clone()]);
    assert_eq!(v.programs.len(), PROGRAMS.len(), "each computer lists every supported program");
    let states: BTreeMap<_, _> = v.programs.iter().map(|p| (p.program.as_str(), p.state)).collect();
    assert_eq!(states["blender"], ProgramState::Ready);
    assert_eq!(states["freecad"], ProgramState::NotInstalled);
    assert_eq!(serde_json::to_value(ProgramState::NotEnabled).unwrap(), json!("not_enabled"));
    let text = programs_text(&[v, view(&device("laptop01", "Mo's laptop", &[]), false, &[r])]);
    assert!(text.contains("Mo's desk: Blender: ready") && text.contains("Mo's laptop (not connected now): Blender: not enabled"), "{text}");
}

/// A report keeps only the supported programs, named tools and bounded schemas.
#[test]
fn a_report_keeps_only_what_the_bridge_supports() {
    let mut odd = reported("blender", true, &["run_python", "bad name", "a__b", "run_python"]);
    odd.tools.push(ReportedTool { name: "big".into(), description: String::new(), input_schema: json!({ "x": "y".repeat(MAX_SCHEMA_BYTES) }) });
    odd.tools.push(ReportedTool { name: "plain".into(), description: "d".repeat(MAX_DESCRIPTION_CHARS + 5), input_schema: json!("not an object") });
    let kept = clean_report(vec![odd, reported("photoshop", true, &["x"]), reported("blender", false, &[])]);
    assert_eq!(kept.len(), 1, "an unsupported program or a second Blender was kept");
    let names: Vec<&str> = kept[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["run_python", "plain"]);
    assert_eq!(kept[0].tools[1].description.chars().count(), MAX_DESCRIPTION_CHARS);
    assert!(kept[0].tools[1].input_schema.is_object());
}

/// A call goes to the computer where the program is ready, or the one
/// named; otherwise it is refused with where the program stands.
#[test]
fn a_call_is_routed_to_the_computer_where_its_program_is_ready() {
    let ready = reported("blender", true, &["run_python"]);
    let laptop = view(&device("laptop01", "Mo's laptop", &[]), true, &[ready.clone()]);
    let desk = view(&device("desk0001", "Mo's desk", &["blender"]), true, &[ready.clone()]);
    assert_eq!(pick(&[laptop.clone(), desk.clone()], "blender", None).unwrap(), "desk0001");
    assert_eq!(pick(&[laptop.clone(), desk.clone()], "blender", Some("Mo's desk")).unwrap(), "desk0001");
    let why = pick(&[laptop.clone(), desk.clone()], "blender", Some("Mo's laptop")).unwrap_err();
    assert!(why.contains("not enabled on Mo's laptop"), "{why}");
    let why = pick(&[laptop.clone()], "freecad", None).unwrap_err();
    assert!(why.contains("not installed on Mo's laptop"), "{why}");
    assert!(pick(&[], "blender", None).unwrap_err().contains("no computer"));
    assert!(pick(&[desk.clone()], "blender", Some("Ana's")).unwrap_err().contains("called Ana's"));
    let other_desk = view(&device("desk0002", "Mo's other desk", &["blender"]), true, &[ready.clone()]);
    let why = pick(&[desk.clone(), other_desk], "blender", None).unwrap_err();
    assert!(why.contains("name one"), "{why}");
    let closed = view(&device("desk0001", "Mo's desk", &["blender"]), true, &[reported("blender", false, &[])]);
    assert!(pick(&[closed], "blender", Some("Mo's desk")).unwrap_err().contains("closed on Mo's desk"));
}

/// The agent sees a ready program's tools, with the computers it is ready on.
#[test]
fn the_agent_sees_tools_only_of_ready_programs() {
    let ready = reported("blender", true, &["run_python"]);
    let desk = view(&device("desk0001", "Mo's desk", &["blender"]), true, &[ready.clone()]);
    let laptop = view(&device("laptop01", "Mo's laptop", &[]), true, &[ready.clone()]);
    let off = view(&device("old00001", "Mo's old one", &["blender"]), false, &[ready]);
    let s = specs(&[desk, laptop, off]);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].name, "desktop_blender__run_python");
    assert_eq!(s[0].input_schema["properties"]["computer"]["enum"], json!(["Mo's desk"]));
    assert!(s[0].description.starts_with("In Blender on the person's computer"));
}

#[test]
fn a_program_tool_runs_code_unless_classified_otherwise() {
    let f = facts("desktop_blender", "run_python").unwrap();
    assert_eq!((f.risk, f.label, f.audience), (Risk::Irreversible, BOTH, Audience::Private));
    assert!(f.connector.is_none() && !f.reaches_self);
    assert!(facts("desktop_photoshop", "x").is_none() && facts("blender", "x").is_none() && facts("desktop_blender", "a b").is_none());
    let p = Program { id: "blender", title: "Blender", classified: &[("list_objects", Risk::Read)] };
    assert!(executes_code(&p, "run_python") && !executes_code(&p, "list_objects"));
    assert_eq!(crate::asks::Reason::of(Some(ASK_RULE)), Some(crate::asks::Reason::AskFirst), "the ask is not one of the four");
}

fn read_tool() -> Facts {
    Facts { risk: Risk::Read, ..facts("desktop_blender", "list_objects").unwrap() }
}

/// The default policy asks the person before code runs in a program.
#[test]
fn by_default_code_in_a_program_asks_the_person() {
    let f = facts("desktop_blender", "run_python").unwrap();
    let out = decide(&BTreeMap::new(), &person("mo", &[]), None, &ToolFacts::from(&f), false, false).unwrap();
    assert!(matches!(out, Outcome::Ask { .. }), "{out:?}");
    let (why, rule) = ask_of(&out, &f).unwrap().unwrap();
    assert_eq!(rule.as_deref(), Some(ASK_RULE));
    assert!(why.contains("runs code inside Blender"), "{why}");
    let out = decide(&BTreeMap::new(), &person("mo", &[]), None, &ToolFacts::from(&read_tool()), false, false).unwrap();
    assert!(ask_of(&out, &read_tool()).is_none(), "a read asked by default");
}

const RELAX_DESIGN: &str = r#"
permit(principal, action == Action::"execute", resource) when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" };
permit(principal, action == Action::"request_approval", resource);
permit(principal in Group::"design", action == Action::"execute", resource in App::"desktop_blender");
"#;

/// The organisation's layer may relax it, for everyone or a group.
#[test]
fn an_admin_relaxes_it_for_a_group() {
    let f = facts("desktop_blender", "run_python").unwrap();
    let policies = BTreeMap::from([("org".to_string(), RELAX_DESIGN.to_string())]);
    let designer = decide(&policies, &person("ana", &["design"]), None, &ToolFacts::from(&f), false, false).unwrap();
    assert!(matches!(designer, Outcome::Allow { .. }) && ask_of(&designer, &f).is_none(), "{designer:?}");
    let other = decide(&policies, &person("mo", &["sales"]), None, &ToolFacts::from(&f), false, false).unwrap();
    assert!(matches!(other, Outcome::Ask { .. }), "the relaxation reached beyond its group: {other:?}");
}

/// A person may only make it stricter (ruling 5): their own permit is
/// refused when written and does nothing if present; their forbid asks
/// or refuses.
#[test]
fn a_person_can_only_tighten_it() {
    let f = facts("desktop_blender", "run_python").unwrap();
    let loosen = r#"permit(principal, action == Action::"execute", resource in App::"desktop_blender");"#;
    assert!(crate::policy_layers::tightens_only("user/mo", loosen).is_err(), "a person's permit was accepted");
    let policies = BTreeMap::from([("user/mo".to_string(), loosen.to_string())]);
    let out = decide(&policies, &person("mo", &[]), None, &ToolFacts::from(&f), false, false).unwrap();
    assert!(matches!(out, Outcome::Ask { .. }), "a person's own permit loosened the ask: {out:?}");

    let ask_first = r#"forbid(principal, action == Action::"execute", resource in App::"desktop_blender");"#;
    let policies = BTreeMap::from([("user/mo".to_string(), ask_first.to_string())]);
    let out = decide(&policies, &person("mo", &[]), None, &ToolFacts::from(&read_tool()), false, false).unwrap();
    assert!(matches!(out, Outcome::Ask { .. }) && matches!(ask_of(&out, &read_tool()), Some(Ok(_))), "a person's forbid did not make a read ask: {out:?}");
    let never = r#"forbid(principal, action, resource in App::"desktop_blender");"#;
    let both = BTreeMap::from([("org".to_string(), RELAX_DESIGN.to_string()), ("user/ana".to_string(), never.to_string())]);
    let out = decide(&both, &person("ana", &["design"]), None, &ToolFacts::from(&f), false, false).unwrap();
    assert!(matches!(ask_of(&out, &f), Some(Err(_))), "a person's forbid did not hold over the org's relaxation: {out:?}");
}

#[path = "desktop_e2e_tests.rs"]
mod e2e;
