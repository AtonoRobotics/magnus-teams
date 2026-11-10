//! The tools for what the agent shows, and the records the loop writes itself.
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use magnus_adapter_proto::ToolSpec;
use serde_json::json;

/// The prompt goes to the image provider, so it passes the boundary.
pub const BUILTINS: &[Builtin] = &[
    Builtin { name: crate::ui::images::GENERATE_TOOL, app: "images", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Public },
    Builtin { name: crate::ui::images::EDIT_TOOL, app: "images", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Public },
];

/// Run records the loop writes to the log itself, outside execute() (K21).
pub const RECORDS: &[&str] = &[
    "show",
    "ui_catalog",
    crate::ui::docs::EDIT_TOOL,
    crate::ui::docs::SCREENSHOT_TOOL,
    "task_contract",
    "task_plan",
    "task_complete",
    "task_fail",
    "ask_principal",
    "propose_policy",
    "end_schedule",
    "wait_until",
    "notify",
    "observe",
    "interrupt",
];

const CATALOG: &str = "Rich answers. Most answers are plain text. To show a card, form, chart, table, gallery or dashboard, write a fenced block in your answer that starts ```magnus-ui, optionally with id=NAME, one element per line: `stat label=Revenue value=41.7 unit=M`. Indented lines belong to the element above. For what the catalog cannot express, write a ```magnus-html block: it runs in a sandbox with no network. Use this tool with an element name to read how it is written; with none, to list the elements.";
const SCREENSHOT: &str = "See how a named magnus-html block you wrote looks, as the person will see it.";
const IMAGE: &str = "Make a picture. You get a placeholder at once and carry on: write `image placeholder=ID` in a spec and it appears when ready. quality is fast by default; use premium only when asked. Diagrams are SVG in magnus-html and charts are specs, not pictures.";
const IMAGE_EDIT: &str = "Change a picture made with image_generate, named by its placeholder. Also answered with a placeholder at once.";
const EDIT: &str = "Change a named magnus-ui or magnus-html block with a search and replace, rather than writing it again. `find` must appear exactly once. A block of under about 300 tokens is better rewritten whole.";

pub(crate) fn specs() -> Vec<ToolSpec> {
    let quality = json!({ "type": "string", "enum": ["fast", "premium"] });
    vec![
        ToolSpec {
            name: crate::ui::images::GENERATE_TOOL.into(),
            description: IMAGE.into(),
            input_schema: obj(json!({ "prompt": { "type": "string" }, "quality": quality }), &["prompt"]),
        },
        ToolSpec {
            name: crate::ui::images::EDIT_TOOL.into(),
            description: IMAGE_EDIT.into(),
            input_schema: obj(json!({ "image": { "type": "string" }, "prompt": { "type": "string" }, "quality": quality }), &["image", "prompt"]),
        },
        ToolSpec { name: crate::ui::docs::SCREENSHOT_TOOL.into(), description: SCREENSHOT.into(), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec { name: "ui_catalog".into(), description: CATALOG.into(), input_schema: obj(json!({ "element": { "type": "string" } }), &[]) },
        ToolSpec {
            name: crate::ui::docs::EDIT_TOOL.into(),
            description: EDIT.into(),
            input_schema: obj(json!({ "id": { "type": "string" }, "find": { "type": "string" }, "replace": { "type": "string" } }), &["id", "find", "replace"]),
        },
    ]
}
