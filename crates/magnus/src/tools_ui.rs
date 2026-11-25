//! The tools for what the agent shows, and the records the loop writes itself.
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use magnus_adapter_proto::ToolSpec;
use serde_json::json;

/// The prompt goes to the image provider, so it passes the boundary.
pub const BUILTINS: &[Builtin] = &[
    Builtin { name: crate::ui::images::GENERATE_TOOL, app: "images", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Public },
    Builtin { name: crate::ui::images::EDIT_TOOL, app: "images", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Public },
];

/// The rich-answer tools: run records the loop writes to the log itself,
/// outside execute() (K21), declared here with their specs.
pub const RECORDS: &[&str] = &["ui_catalog", crate::ui::docs::EDIT_TOOL, crate::ui::docs::SCREENSHOT_TOOL];

/// A task's contract, retired at rev 147: its stored calls still read as a record.
pub const RETIRED_CONTRACT: &str = "task_contract";

const CATALOG: &str = "Rich, composable answers with zero extra round-trips. Use clean Markdown (headings, bold, tables, callouts `> [!TIP]`, code blocks) and embed compact ```magnus-ui fenced blocks whenever showing metrics, charts, comparisons, timelines, or research sources (each block is ~40 tokens and streams live): `stat label=Revenue value=41.7 unit=M delta=\"+18%\"` (consecutive stats auto-group into a KPI strip), `bar_chart title=\"...\" labels=A,B values=10,20 unit=M`, `line_chart title=\"...\" labels=Jan,Feb` + indented `series Name values=1,2`, `comparison options=A,B` + indented `aspect Speed values=1,2 best=0`, `timeline` + indented `event when=2026 label=\"...\"`, `callout tone=tip title=\"Takeaway\" text=\"...\"`, `sources` + indented `source title=\"...\" url=\"https://...\"`. For bespoke interactive widgets or SVG diagrams that the catalog cannot express, write a ```magnus-html block (sandboxed, auto-sized, theme CSS vars injected). Call this tool only if you need docs for other elements (card, deck, form, gallery, map, table, action).";
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
