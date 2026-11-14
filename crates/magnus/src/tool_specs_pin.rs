//! The built-in tool specs the model sees, pinned byte for byte: names,
//! descriptions, input schemas and order, for every set a run can be offered.
//! Prompt caching and evals depend on them, so a change to one is a
//! deliberate change to `tool_specs.snapshot.json`, made by running
//! `cargo test -p magnus --lib print_the_tool_specs_snapshot -- --ignored --nocapture`
//! and copying what it prints between the markers.
use magnus_adapter_proto::ToolSpec;
use serde_json::{json, Map, Value};

const PINNED: &str = include_str!("tool_specs.snapshot.json");
const BEGIN: &str = "----- tool specs snapshot -----";
const END: &str = "----- end of tool specs snapshot -----";

/// Every list of specs a run can be offered, by what decides it.
fn lists() -> Vec<(String, Vec<ToolSpec>)> {
    let mut v = vec![];
    for bits in 0..16u8 {
        let (sandbox, search, browser, computer) = (bits & 8 != 0, bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
        v.push((format!("builtin sandbox={sandbox} search={search} browser={browser} computer={computer}"), crate::tools::builtin_specs(sandbox, search, browser, computer)));
    }
    v.push(("chat".into(), crate::tools::chat_specs()));
    v.push(("find_tools".into(), vec![crate::tools::find_tools_spec()]));
    v.push(("team".into(), crate::team::specs()));
    v
}

/// Each list's names in order, and each spec once, as the bytes it is sent as.
fn snapshot() -> Value {
    let (mut specs, mut orders) = (Map::new(), Map::new());
    for (list, v) in lists() {
        for s in &v {
            let bytes = Value::String(serde_json::to_string(s).unwrap());
            if let Some(before) = specs.insert(s.name.clone(), bytes.clone()) {
                assert_eq!(before, bytes, "{} is sent differently in {list}", s.name);
            }
        }
        orders.insert(list, v.iter().map(|s| json!(s.name)).collect());
    }
    json!({ "lists": orders, "specs": specs })
}

#[test]
fn the_model_visible_tool_specs_are_pinned() {
    let pinned: Value = serde_json::from_str(PINNED).unwrap();
    let now = snapshot();
    for (list, names) in pinned["lists"].as_object().unwrap() {
        assert_eq!(&now["lists"][list], names, "the tools offered in {list}, or their order, changed");
    }
    assert_eq!(now["lists"].as_object().unwrap().len(), pinned["lists"].as_object().unwrap().len(), "a list was added");
    for (name, bytes) in pinned["specs"].as_object().unwrap() {
        assert_eq!(&now["specs"][name], bytes, "the spec of {name} changed");
    }
    assert_eq!(now["specs"].as_object().unwrap().len(), pinned["specs"].as_object().unwrap().len(), "a spec was added");
}

#[test]
#[ignore = "prints the snapshot, to pin a deliberate change"]
fn print_the_tool_specs_snapshot() {
    println!("{BEGIN}\n{}\n{END}", serde_json::to_string_pretty(&snapshot()).unwrap());
}
