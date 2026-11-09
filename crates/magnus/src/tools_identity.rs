//! The specs of the tools that act as a person's signed-in identity.
use crate::tools::obj;
use magnus_adapter_proto::ToolSpec;
use serde_json::{json, Value};

pub(crate) fn specs(d: &dyn Fn(&str) -> String) -> Vec<ToolSpec> {
    let mut v = vec![];
    let identity = json!({ "type": "string", "description": "the identity's id" });
    let target = json!({ "type": "string", "description": "an element ref like e12 from the latest snapshot, or visible text" });
    let as_tool = |name: &str, extra: Value, needs: &[&str]| {
        let mut props = json!({ "identity": identity });
        if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        let mut required = vec!["identity"];
        required.extend(needs);
        ToolSpec { name: format!("browser_as_{name}"), description: d(&format!("browser_as_{name}")), input_schema: obj(props, &required) }
    };
    v.push(as_tool("navigate", json!({ "url": { "type": "string" } }), &["url"]));
    v.push(as_tool("snapshot", json!({}), &[]));
    v.push(as_tool("click", json!({ "target": target }), &["target"]));
    v.push(as_tool("type", json!({ "target": target, "text": { "type": "string" }, "submit": { "type": "boolean" } }), &["target", "text"]));
    v.push(as_tool("select", json!({ "target": target, "value": { "type": "string" } }), &["target", "value"]));
    v.push(as_tool("back", json!({}), &[]));
    v.push(as_tool("screenshot", json!({}), &[]));
    v.push(ToolSpec {
        name: "browser_sign_in".into(),
        description: d("browser_sign_in"),
        input_schema: obj(json!({ "identity": identity, "username_field": target, "password_field": target }), &["identity", "password_field"]),
    });
    v
}

pub(crate) fn sub_agent_spec(d: &dyn Fn(&str) -> String) -> ToolSpec {
    ToolSpec {
        name: "start_sub_agent".into(),
        description: d("start_sub_agent"),
        input_schema: obj(
            json!({ "goal": { "type": "string" }, "tools": { "type": "array", "items": { "type": "string" } }, "grant": { "type": "string" }, "budget_share": { "type": "number" } }),
            &["goal"],
        ),
    }
}
