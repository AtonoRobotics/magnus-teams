//! Tool vocabulary: risk classes, inbound labels, audiences, and the built-in
//! tools every run is offered. App tools come from MCP servers, named
//! `<app>__<tool>`, and are classified by the owner in the config.

use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A tool's reach (approvals spec, The approval model): `self` stays in the
/// person's own account (a draft, a label, the trash) and reaches no one, so
/// neither the Rule of Two nor the audience rule counts it as sending, as
/// for a write to Magnus's memory. A tool that declares no reach is
/// unknown, and asks as before. Money never counts as `self`.
pub const REACH_SELF: &str = "self";

/// Whether a declared reach is one Magnus knows, on a tool of this class:
/// `self` on a write; never on a read (nothing to reach), money (it asks
/// every time) or a prohibited tool.
pub fn reach_fits(reach: &str, risk: &str) -> bool {
    reach == REACH_SELF && ["reversible_write", "irreversible"].contains(&risk)
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Read,
    /// Changes only the run's own workspace, which has no network (K42).
    Sandbox,
    ReversibleWrite,
    Irreversible,
    Financial,
    Prohibited,
}

impl Risk {
    pub fn parse(s: &str) -> Option<Risk> {
        Some(match s {
            "read" => Risk::Read,
            "sandbox" => Risk::Sandbox,
            "reversible_write" => Risk::ReversibleWrite,
            "irreversible" => Risk::Irreversible,
            "financial" => Risk::Financial,
            "prohibited" => Risk::Prohibited,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Sandbox => "sandbox",
            Risk::ReversibleWrite => "reversible_write",
            Risk::Irreversible => "irreversible",
            Risk::Financial => "financial",
            Risk::Prohibited => "prohibited",
        }
    }
    /// State change or external communication, for the Rule of Two (K34).
    pub fn changes_state(self) -> bool {
        !matches!(self, Risk::Read | Risk::Sandbox)
    }
}

/// How content is labeled on the way in (B3). A source can be both private
/// and untrusted: a mailbox holds the person's private mail, written by others.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Label {
    pub untrusted: bool,
    pub private: bool,
}

impl Label {
    pub const PRINCIPAL: Label = Label { untrusted: false, private: false };
    pub const PRIVATE: Label = Label { untrusted: false, private: true };
    pub const UNTRUSTED: Label = Label { untrusted: true, private: false };

    /// `principal`, `private`, `untrusted`, or a combination like `private+untrusted`.
    pub fn parse(s: &str) -> Option<Label> {
        let mut l = Label::PRINCIPAL;
        for part in s.split(['+', ',', ' ']).filter(|p| !p.is_empty()) {
            match part {
                "principal" => {}
                "private" => l.private = true,
                "untrusted" => l.untrusted = true,
                _ => return None,
            }
        }
        Some(l)
    }

    pub fn join(self, o: Label) -> Label {
        Label { untrusted: self.untrusted || o.untrusted, private: self.private || o.private }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Private,
    Internal,
    Public,
}

impl Audience {
    pub fn parse(s: &str) -> Option<Audience> {
        Some(match s {
            "private" => Audience::Private,
            "internal" => Audience::Internal,
            "public" => Audience::Public,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::Private => "private",
            Audience::Internal => "internal",
            Audience::Public => "public",
        }
    }
}

pub use crate::tools_ui::RECORDS;

/// Built-in actions that pass the boundary like app tools.
pub struct Builtin {
    pub name: &'static str,
    pub app: &'static str,
    pub risk: Risk,
    pub label: Label,
    pub audience: Audience,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin { name: "sandbox_exec", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "sandbox_write_file", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "sandbox_read_file", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "web_fetch", app: "web", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "web_search", app: "web", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_navigate", app: "browser", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_snapshot", app: "browser", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_screenshot", app: "browser", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_back", app: "browser", risk: Risk::Read, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_click", app: "browser", risk: Risk::ReversibleWrite, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_type", app: "browser", risk: Risk::ReversibleWrite, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_select", app: "browser", risk: Risk::ReversibleWrite, label: Label::UNTRUSTED, audience: Audience::Public },
    Builtin { name: "browser_as_navigate", app: "browser", risk: Risk::Read, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_snapshot", app: "browser", risk: Risk::Read, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_screenshot", app: "browser", risk: Risk::Read, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_back", app: "browser", risk: Risk::Read, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_click", app: "browser", risk: Risk::ReversibleWrite, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_type", app: "browser", risk: Risk::ReversibleWrite, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_as_select", app: "browser", risk: Risk::ReversibleWrite, label: Label { untrusted: true, private: true }, audience: Audience::Private },
    Builtin { name: "browser_sign_in", app: "browser", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "start_sub_agent", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "open_attachment", app: "sandbox", risk: Risk::Sandbox, label: Label::UNTRUSTED, audience: Audience::Private },
    Builtin { name: "deliver_file", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "computer_screenshot", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "add_computer_destination", app: "computer", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Public },
    Builtin { name: "computer_click", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "computer_type", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "computer_key", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "computer_scroll", app: "sandbox", risk: Risk::Sandbox, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "memory_write", app: "memory", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "memory_search", app: "memory", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // The person's relationship memory (behavior spec, Per-user
    // relationship): their own notes, on the appliance, versioned.
    Builtin { name: "relationship_note", app: "memory", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // The person's disagreement log (behavior spec): theirs, on the appliance.
    Builtin { name: "disagreement", app: "memory", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // The person's commitment ledger (#236): what they and Magnus said
    // they would do, recorded from their words or what a run read; theirs.
    Builtin { name: "record_commitment", app: "memory", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "update_commitment", app: "memory", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // The company knowledge base. A search's result carries the taint of
    // what it returns; filing research changes nothing outside the appliance.
    Builtin { name: "knowledge_search", app: "knowledge", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // The org context bundle (org context build spec): reads carry the
    // taint their documents were written under; writes stay on the
    // appliance, versioned and reversible.
    Builtin { name: "bundle_read", app: "bundle", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "bundle_search", app: "bundle", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "bundle_write", app: "bundle", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "bundle_share", app: "bundle", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "knowledge_add", app: "knowledge", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // A project's reference file, read when the work needs it (M2). Its
    // result carries the file's own taint; an upload is private and untrusted.
    Builtin { name: "project_read", app: "library", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "playbook_update", app: "playbook", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // The person's apps (connectors spec, How the agent uses connectors):
    // reading their Library, and putting an app's card in front of them.
    // Only the person signs in; nothing leaves the appliance.
    Builtin { name: "find_apps", app: "apps", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "app_details", app: "apps", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "offer_app", app: "apps", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // Past `LOAD_ALL` tools across the person's connected apps, a run is
    // offered only those it found with this (connectors spec: tools searched
    // across connectors and full schemas loaded on demand).
    Builtin { name: "find_tools", app: "apps", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // Chat only: start a task for the person, or show them a schedule to
    // confirm. Neither leaves the appliance; the task carries the turn's taint.
    Builtin { name: "create_task", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "create_schedule", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // An automation suggestion: a card the person accepts (a task or a
    // schedule starts), puts off or declines. Nothing happens until they accept.
    Builtin { name: "suggest", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // A task proposal: a card for one task the person approves (it starts,
    // within the cost they approved) or dismisses. Nothing starts until then.
    Builtin { name: "propose_task", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    // A grant card, when the person asked for standing authority over a
    // kind of their work (#235): nothing is granted until they accept it.
    // Revoking one of theirs at their word only narrows what Magnus may do.
    Builtin { name: "propose_grant", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "revoke_grant", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    // Control of the person's own tasks and schedules, as the app's buttons
    // (behavior spec, Tasks and schedules in conversation).
    Builtin { name: "list_tasks", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "list_schedules", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "get_task", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "get_schedule", app: "chat", risk: Risk::Read, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "cancel_task", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "pause_schedule", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "resume_schedule", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "edit_schedule", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "cancel_schedule", app: "chat", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
];

pub fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS
        .iter()
        .chain(crate::local::BUILTINS)
        .chain(crate::team::BUILTINS)
        .chain(crate::tools_ui::BUILTINS)
        .chain(crate::generated::BUILTINS)
        .chain(crate::payments::BUILTINS)
        .find(|b| b.name == name)
}

/// Tools whose calls are their person's alone: no other person's run, the
/// owner, an admin or another developer sees their words (behavior spec,
/// Per-user relationship; #232, #233). Reads served to anyone but the run's
/// person redact these calls' inputs and results (#257).
pub fn person_only_tool(name: &str) -> bool {
    matches!(name, "relationship_note" | "disagreement")
}

/// What a redacted call's input and result say, so the call stays listed.
pub const REDACTED_CALL: &str = "redacted: this call is the person's alone";

/// The appliance's own apps; no added app may take one of their names.
pub fn builtin_apps() -> Vec<&'static str> {
    let mut a: Vec<&str> =
        BUILTINS.iter().chain(crate::local::BUILTINS).chain(crate::team::BUILTINS).chain(crate::generated::BUILTINS).chain(crate::payments::BUILTINS).map(|b| b.app).collect();
    a.sort();
    a.dedup();
    a
}

pub(crate) fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

/// Whether a contract check may call this tool: anything that only reads, and
/// of the sandbox tools only running a command and reading a file, which a
/// check does in a VM of its own.
pub fn checkable(name: &str, risk: Risk) -> bool {
    risk == Risk::Read || (risk == Risk::Sandbox && matches!(name, "sandbox_exec" | "sandbox_read_file"))
}

/// The fields a built-in tool's result shows a check: its structured part
/// and `text`, nothing else. A connector's read shows the fields of the JSON
/// it returns, known only when it runs.
pub fn check_fields(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "sandbox_exec" => &["exit_code", "stdout", "stderr", "timed_out", "truncated", "text"],
        "web_fetch" => &["content_type", "text"],
        "sandbox_read_file" | "web_search" => &["text"],
        n if n.starts_with("browser_") => &["image_b64", "text"],
        _ => return None,
    })
}

/// One component of a composition, of the given kinds. Every field is
/// declared: a model that keeps to the schema cannot otherwise fill one in.
fn component(kinds: &[&str]) -> Value {
    let cell = json!({ "type": ["string", "number"] });
    json!({ "type": "object", "properties": {
        "type": { "type": "string", "enum": kinds },
        "text": { "type": "string", "description": "heading, text" },
        "items": { "type": "array", "items": { "type": "string" }, "description": "list" },
        "columns": { "type": "array", "items": { "type": "string" }, "description": "table" },
        "rows": { "type": "array", "items": { "type": "array", "items": cell }, "description": "table: one list of cells per row, as many as columns" },
        "label": { "type": "string", "description": "stat, action: the button's words" }, "request": { "type": "string", "description": "action: the work a press asks for; it starts a new task and approves nothing" },
        "value": { "type": ["string", "number"], "description": "stat" },
        "unit": { "type": "string", "description": "stat, bar_chart" },
        "labels": { "type": "array", "items": { "type": "string" }, "description": "bar_chart, line_chart" },
        "values": { "type": "array", "items": { "type": "number" }, "description": "bar_chart: one per label" },
        "series": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "values": { "type": "array", "items": { "type": "number" } } }, "required": ["name", "values"] }, "description": "line_chart: 1 to 6, one value per label each" },
        "places": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "lat": { "type": "number" }, "lon": { "type": "number" }, "note": { "type": "string" } }, "required": ["name", "lat", "lon"] }, "description": "map" },
        "events": { "type": "array", "items": { "type": "object", "properties": { "when": { "type": "string" }, "label": { "type": "string" }, "note": { "type": "string" } }, "required": ["when", "label"] }, "description": "timeline, in order" },
        "options": { "type": "array", "items": { "type": "string" }, "description": "comparison: the 2 to 6 things compared" },
        "aspects": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "values": { "type": "array", "items": cell }, "best": { "type": "integer" } }, "required": ["name", "values"] }, "description": "comparison: one value per option each; best is the index of the option that wins on it" },
        "code": { "type": "string", "description": "code" },
        "language": { "type": "string", "description": "code" },
        "file": { "type": "string", "description": "image, file: the name of a file you delivered" },
        "title": { "type": "string" },
        "caption": { "type": "string" }
    }, "required": ["type"] })
}

/// The tools a chat turn may call besides reads: start a task, propose a
/// schedule the person confirms in the thread, and control their tasks and
/// schedules.
pub fn chat_specs() -> Vec<ToolSpec> {
    let desc: Value = serde_json::from_str(include_str!("../instructions/tools.json")).expect("tools.json");
    let d = |n: &str| desc[n].as_str().unwrap_or_default().to_string();
    let budget = json!({ "type": "object", "description": d("create_schedule.budget"), "properties": {
        "max_usd": { "type": "number" }, "max_model_calls": { "type": "integer" }, "max_actions": { "type": "integer" }
    } });
    let evidence = json!({ "type": "array", "minItems": 1, "description": d("suggest.evidence"), "items": { "type": "object", "properties": {
        "call": { "type": "string" }, "task": { "type": "string" }, "shows": { "type": "string" }
    }, "required": ["shows"] } });
    vec![
        ToolSpec {
            name: "show".into(),
            description: d("show"),
            input_schema: obj(json!({ "components": { "type": "array", "minItems": 1, "items": component(crate::compose::SHOWN) } }), &["components"]),
        },
        ToolSpec {
            name: "create_task".into(),
            description: d("create_task"),
            input_schema: obj(json!({ "title": { "type": "string" }, "text": { "type": "string" }, "grant": { "type": "string" } }), &["title", "text"]),
        },
        ToolSpec {
            name: "create_schedule".into(),
            description: d("create_schedule"),
            input_schema: obj(
                json!({ "title": { "type": "string" }, "text": { "type": "string" }, "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" }, "scope": { "type": "string" }, "budget": budget }),
                &["title", "text", "scope"],
            ),
        },
        ToolSpec {
            name: "suggest".into(),
            description: d("suggest"),
            input_schema: obj(
                json!({
                    "title": { "type": "string" },
                    "plan": { "type": "string" },
                    "evidence": evidence.clone(),
                    "permissions": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                    "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" },
                    "budget": budget,
                    "again": { "type": "string" }, "changed": { "type": "string" }
                }),
                &["title", "plan", "evidence", "permissions"],
            ),
        },
        ToolSpec {
            name: "propose_task".into(),
            description: d("propose_task"),
            input_schema: obj(
                json!({
                    "title": { "type": "string" },
                    "text": { "type": "string" },
                    "noticed": { "type": "object", "properties": { "what": { "type": "string" }, "where": { "type": "string" } }, "required": ["what", "where"] },
                    "outcome": { "type": "string" },
                    "why_now": { "type": "string" },
                    "needs": { "type": "object", "properties": {
                        "permissions": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                        "estimated_cost_usd": { "type": "number" },
                        "input": { "type": "string" }
                    }, "required": ["permissions", "estimated_cost_usd"] },
                    "evidence": evidence,
                    "again": { "type": "string" }, "changed": { "type": "string" }
                }),
                &["title", "text", "noticed", "outcome", "why_now", "needs", "evidence"],
            ),
        },
        ToolSpec {
            name: "propose_grant".into(),
            description: d("propose_grant"),
            input_schema: obj(
                json!({
                    "title": { "type": "string" },
                    "apps": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                    "allow": { "type": "array", "items": { "type": "string", "enum": crate::grants::LIFTABLE } },
                    "period": { "type": "string", "enum": ["day", "week", "month"] },
                    "max_usd": { "type": "number" },
                    "why": { "type": "string" },
                    "replaces": { "type": "string" }
                }),
                &["title", "apps", "period", "max_usd", "why"],
            ),
        },
        ToolSpec { name: "revoke_grant".into(), description: d("revoke_grant"), input_schema: obj(json!({ "grant": { "type": "string" } }), &["grant"]) },
        ToolSpec {
            name: "bundle_share".into(),
            description: d("bundle_share"),
            input_schema: obj(json!({ "path": { "type": "string" }, "groups": { "type": "array", "items": { "type": "string" } }, "everyone": { "type": "boolean" } }), &["path"]),
        },
        ToolSpec { name: "list_tasks".into(), description: d("list_tasks"), input_schema: obj(json!({ "limit": { "type": "integer" } }), &[]) },
        ToolSpec { name: "list_schedules".into(), description: d("list_schedules"), input_schema: obj(json!({}), &[]) },
        ToolSpec { name: "get_task".into(), description: d("get_task"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec { name: "get_schedule".into(), description: d("get_schedule"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec { name: "cancel_task".into(), description: d("cancel_task"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec { name: "pause_schedule".into(), description: d("pause_schedule"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec { name: "resume_schedule".into(), description: d("resume_schedule"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
        ToolSpec {
            name: "edit_schedule".into(),
            description: d("edit_schedule"),
            input_schema: obj(
                json!({ "id": { "type": "string" }, "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" } }),
                &["id"],
            ),
        },
        ToolSpec { name: "cancel_schedule".into(), description: d("cancel_schedule"), input_schema: obj(json!({ "id": { "type": "string" } }), &["id"]) },
    ]
}

/// Specs for run records and built-in actions. Descriptions are part of the
/// versioned instructions (X11): they live in `instructions/tools.json`.
/// Up to this many tools across a person's connected apps, a run is offered
/// all of them; past it, those it finds with `find_tools`.
pub const LOAD_ALL: usize = 40;

/// `find_tools`, offered only when a run's apps have more tools than `LOAD_ALL`.
pub fn find_tools_spec() -> ToolSpec {
    let desc: Value = serde_json::from_str(include_str!("../instructions/tools.json")).expect("tools.json");
    ToolSpec {
        name: "find_tools".into(),
        description: desc["find_tools"].as_str().unwrap_or_default().to_string(),
        input_schema: json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
    }
}

pub fn builtin_specs(sandbox: bool, search: bool, browser: bool, computer: bool) -> Vec<ToolSpec> {
    let desc: Value = serde_json::from_str(include_str!("../instructions/tools.json")).expect("tools.json");
    let d = |n: &str| desc[n].as_str().unwrap_or_default().to_string();
    let criterion = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "description": { "type": "string" },
            "check": {
                "type": "object",
                "properties": {
                    "tool": { "type": "string" },
                    "input": { "type": "object" },
                    "expect": { "type": "array", "items": { "type": "object", "properties": {
                        "pointer": { "type": "string" }, "equals": {}, "contains": { "type": "string" }
                    }, "required": ["pointer"] } }
                },
                "required": ["tool", "input", "expect"]
            }
        },
        "required": ["id", "description"]
    });
    let mut v = vec![
        ToolSpec { name: "find_apps".into(), description: d("find_apps"), input_schema: obj(json!({ "query": { "type": "string" } }), &[]) },
        ToolSpec { name: "app_details".into(), description: d("app_details"), input_schema: obj(json!({ "name": { "type": "string" } }), &["name"]) },
        ToolSpec {
            name: "offer_app".into(),
            description: d("offer_app"),
            input_schema: obj(json!({ "name": { "type": "string" }, "why": { "type": "string" } }), &["name", "why"]),
        },
        ToolSpec {
            name: "task_contract".into(),
            description: d("task_contract"),
            input_schema: obj(
                json!({
                    "goal": { "type": "string" },
                    "assumptions": { "type": "array", "items": { "type": "string" } },
                    "criteria": { "type": "array", "items": criterion, "minItems": 1 },
                    "irreversible_actions": { "type": "array", "items": { "type": "string" } },
                    "title": { "type": "string" }
                }),
                &["title", "goal", "criteria"],
            ),
        },
        ToolSpec {
            name: "task_plan".into(),
            description: d("task_plan"),
            input_schema: obj(
                json!({ "units": { "type": "array", "minItems": 1, "items": { "type": "object", "properties": {
                    "id": { "type": "string" }, "goal": { "type": "string" }, "done_when": { "type": "string" },
                    "status": { "type": "string", "enum": ["pending", "active", "done", "dropped"] }
                }, "required": ["id", "goal", "done_when"] } } }),
                &["units"],
            ),
        },
        ToolSpec {
            name: "task_complete".into(),
            description: d("task_complete"),
            input_schema: obj(
                json!({
                    "summary": { "type": "string", "description": d("task_complete.summary") },
                    // Every component field is declared: a model that keeps to
                    // the schema cannot otherwise fill one in.
                    "deliverable": { "type": "array", "minItems": 1, "description": d("task_complete.deliverable"), "items": component(crate::compose::KINDS) },
                    "spoken": { "type": "string", "description": d("task_complete.spoken") }
                }),
                &["deliverable", "summary"],
            ),
        },
        ToolSpec { name: "task_fail".into(), description: d("task_fail"), input_schema: obj(json!({ "reason": { "type": "string" } }), &["reason"]) },
        ToolSpec {
            name: "ask_principal".into(),
            description: d("ask_principal"),
            input_schema: obj(json!({ "question": { "type": "string" }, "options": { "type": "array", "items": { "type": "string" } } }), &["question"]),
        },
        ToolSpec {
            name: "propose_policy".into(),
            description: d("propose_policy"),
            input_schema: obj(json!({ "layer": { "type": "string" }, "cedar": { "type": "string" }, "rationale": { "type": "string" } }), &["layer", "cedar", "rationale"]),
        },
        ToolSpec {
            name: "propose_schedule".into(),
            description: d("propose_schedule"),
            input_schema: obj(
                json!({ "text": { "type": "string" }, "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" } }),
                &["text"],
            ),
        },
        ToolSpec { name: "end_schedule".into(), description: d("end_schedule"), input_schema: obj(json!({ "reason": { "type": "string" } }), &["reason"]) },
        ToolSpec {
            name: "wait_until".into(),
            description: d("wait_until"),
            input_schema: obj(json!({ "at": { "type": "string" }, "reason": { "type": "string" } }), &["at", "reason"]),
        },
        ToolSpec { name: "notify".into(), description: d("notify"), input_schema: obj(json!({ "message": { "type": "string" } }), &["message"]) },
        ToolSpec {
            name: "observe".into(),
            description: d("observe"),
            input_schema: obj(json!({ "what": { "type": "string" }, "why": { "type": "string" }, "evidence": { "type": "string" } }), &["what", "why", "evidence"]),
        },
        ToolSpec {
            name: "interrupt".into(),
            description: d("interrupt"),
            input_schema: obj(json!({ "what": { "type": "string" }, "why": { "type": "string" } }), &["what", "why"]),
        },
        ToolSpec { name: "web_fetch".into(), description: d("web_fetch"), input_schema: obj(json!({ "url": { "type": "string" } }), &["url"]) },
        ToolSpec {
            name: "memory_write".into(),
            description: d("memory_write"),
            input_schema: obj(json!({ "fact": { "type": "string" }, "topic": { "type": "string" } }), &["fact"]),
        },
        ToolSpec {
            name: "relationship_note".into(),
            description: d("relationship_note"),
            input_schema: obj(
                json!({ "action": { "type": "string", "enum": ["add", "change", "remove"] }, "note": { "type": "string" }, "text": { "type": "string" } }),
                &["action"],
            ),
        },
        ToolSpec {
            name: "disagreement".into(),
            description: d("disagreement"),
            input_schema: obj(
                json!({ "action": { "type": "string", "enum": ["record", "outcome"] }, "position": { "type": "string" }, "decision": { "type": "string" }, "id": { "type": "string" }, "outcome": { "type": "string" } }),
                &["action"],
            ),
        },
        ToolSpec {
            name: "record_commitment".into(),
            description: d("record_commitment"),
            input_schema: obj(
                json!({
                    "who": { "type": "string", "enum": ["person", "magnus"] }, "what": { "type": "string" }, "to": { "type": "string" },
                    "by": { "type": "string" }, "quote": { "type": "string" }, "call": { "type": "string" }
                }),
                &["who", "what", "quote"],
            ),
        },
        ToolSpec {
            name: "update_commitment".into(),
            description: d("update_commitment"),
            input_schema: obj(json!({ "id": { "type": "string" }, "status": { "type": "string", "enum": ["done", "released"] }, "note": { "type": "string" } }), &["id", "status"]),
        },
        ToolSpec { name: "bundle_read".into(), description: d("bundle_read"), input_schema: obj(json!({ "path": { "type": "string" } }), &["path"]) },
        ToolSpec {
            name: "bundle_search".into(),
            description: d("bundle_search"),
            input_schema: obj(json!({ "query": { "type": "string" }, "kinds": { "type": "array", "items": { "type": "string" } }, "limit": { "type": "integer" } }), &["query"]),
        },
        ToolSpec {
            name: "bundle_write".into(),
            description: d("bundle_write"),
            input_schema: obj(json!({ "path": { "type": "string" }, "frontmatter": { "type": "object" }, "body": { "type": "string" } }), &["path", "frontmatter", "body"]),
        },
        ToolSpec { name: "memory_search".into(), description: d("memory_search"), input_schema: obj(json!({ "query": { "type": "string" } }), &["query"]) },
        ToolSpec { name: "knowledge_search".into(), description: d("knowledge_search"), input_schema: obj(json!({ "query": { "type": "string" } }), &["query"]) },
        ToolSpec {
            name: "knowledge_add".into(),
            description: d("knowledge_add"),
            input_schema: obj(
                json!({
                    "title": { "type": "string" },
                    "text": { "type": "string", "description": d("knowledge_add.text") },
                    "citations": { "type": "array", "minItems": 1, "description": d("knowledge_add.citations"), "items": { "type": "object", "properties": {
                        "source": { "type": "string" }, "quote": { "type": "string" }
                    }, "required": ["source", "quote"] } },
                    "value": { "type": "object", "description": d("knowledge_add.value"), "properties": {
                        "key": { "type": "string" }, "type": { "type": "string", "enum": ["number", "boolean", "enum"] }, "options": { "type": "array", "items": { "type": "string" } }
                    }, "required": ["key", "type"] },
                    "propose": { "type": "boolean", "description": d("knowledge_add.propose") }
                }),
                &["title", "text", "citations"],
            ),
        },
        ToolSpec {
            name: "project_read".into(),
            description: d("project_read"),
            input_schema: obj(json!({ "name": { "type": "string", "description": "the file's name, as the project lists it" } }), &["name"]),
        },
        ToolSpec {
            name: "playbook_update".into(),
            description: d("playbook_update"),
            input_schema: obj(
                json!({
                    "op": { "type": "string", "enum": ["add", "edit", "retire"] },
                    "item": { "type": "string", "description": "the item id, for edit and retire" },
                    "text": { "type": "string" },
                    "addresses": { "type": "string", "description": "the failure this item addresses" }
                }),
                &["op"],
            ),
        },
    ];
    if search {
        v.push(ToolSpec { name: "web_search".into(), description: d("web_search"), input_schema: obj(json!({ "query": { "type": "string" } }), &["query"]) });
    }
    if sandbox {
        v.push(ToolSpec {
            name: "open_attachment".into(),
            description: d("open_attachment"),
            input_schema: obj(json!({ "name": { "type": "string" }, "command": { "type": "string", "description": "optional: a command to run on the file, which is at /workspace/inputs/attachment" } }), &["name"]),
        });
    }
    v.push(crate::tools_identity::sub_agent_spec(&d));
    if browser {
        let target = json!({ "type": "string", "description": "an element ref like e12 from the latest snapshot, or visible text" });
        v.push(ToolSpec { name: "browser_navigate".into(), description: d("browser_navigate"), input_schema: obj(json!({ "url": { "type": "string" } }), &["url"]) });
        v.push(ToolSpec { name: "browser_snapshot".into(), description: d("browser_snapshot"), input_schema: obj(json!({}), &[]) });
        v.push(ToolSpec { name: "browser_click".into(), description: d("browser_click"), input_schema: obj(json!({ "target": target }), &["target"]) });
        v.push(ToolSpec {
            name: "browser_type".into(),
            description: d("browser_type"),
            input_schema: obj(json!({ "target": target, "text": { "type": "string" }, "submit": { "type": "boolean" } }), &["target", "text"]),
        });
        v.push(ToolSpec {
            name: "browser_select".into(),
            description: d("browser_select"),
            input_schema: obj(json!({ "target": target, "value": { "type": "string" } }), &["target", "value"]),
        });
        v.push(ToolSpec { name: "browser_back".into(), description: d("browser_back"), input_schema: obj(json!({}), &[]) });
        v.push(ToolSpec { name: "browser_screenshot".into(), description: d("browser_screenshot"), input_schema: obj(json!({}), &[]) });
    }
    if computer {
        v.extend(crate::tools_identity::specs(&d));
        v.push(ToolSpec {
            name: "add_computer_destination".into(),
            description: d("add_computer_destination"),
            input_schema: obj(json!({ "host": { "type": "string" } }), &["host"]),
        });
    }
    if sandbox {
        let xy = json!({ "type": "integer" });
        v.push(ToolSpec { name: "computer_screenshot".into(), description: d("computer_screenshot"), input_schema: obj(json!({}), &[]) });
        v.push(ToolSpec {
            name: "computer_click".into(),
            description: d("computer_click"),
            input_schema: obj(json!({ "x": xy, "y": xy, "button": { "type": "string", "enum": ["left", "right", "middle"] }, "double": { "type": "boolean" } }), &["x", "y"]),
        });
        v.push(ToolSpec { name: "computer_type".into(), description: d("computer_type"), input_schema: obj(json!({ "text": { "type": "string" } }), &["text"]) });
        v.push(ToolSpec { name: "computer_key".into(), description: d("computer_key"), input_schema: obj(json!({ "keys": { "type": "string" } }), &["keys"]) });
        v.push(ToolSpec {
            name: "computer_scroll".into(),
            description: d("computer_scroll"),
            input_schema: obj(json!({ "x": xy, "y": xy, "down": { "type": "integer" } }), &["x", "y", "down"]),
        });
        v.push(ToolSpec {
            name: "deliver_file".into(),
            description: d("deliver_file"),
            input_schema: obj(json!({ "path": { "type": "string" }, "name": { "type": "string" } }), &["path"]),
        });
        v.push(ToolSpec {
            name: "sandbox_exec".into(),
            description: d("sandbox_exec"),
            input_schema: obj(json!({ "command": { "type": "string" }, "timeout_s": { "type": "integer" } }), &["command"]),
        });
        v.push(ToolSpec {
            name: "sandbox_write_file".into(),
            description: d("sandbox_write_file"),
            input_schema: obj(json!({ "path": { "type": "string" }, "content": { "type": "string" }, "base64": { "type": "boolean" } }), &["path", "content"]),
        });
        v.push(ToolSpec { name: "sandbox_read_file".into(), description: d("sandbox_read_file"), input_schema: obj(json!({ "path": { "type": "string" } }), &["path"]) });
    }
    v
}

/// The addresses in a recipients field, lowercased, as they match a grant
/// (approvals spec, Email end to end: exact addresses, and a mailing list
/// is one). It reads a header (`Ann <ann@x.example>, "Doe, Bo" <bo@y>`),
/// a list of them, or objects carrying an `email` or `address`.
pub fn addresses(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Value::String(s) => {
            let (mut part, mut quoted, mut angle) = (String::new(), false, false);
            for ch in s.chars().chain([',']) {
                match ch {
                    '"' if !angle => quoted = !quoted,
                    '<' if !quoted => angle = true,
                    '>' if !quoted => angle = false,
                    ',' | ';' if !quoted && !angle => {
                        if let Some(a) = address(&part) {
                            out.push(a);
                        }
                        part.clear();
                        continue;
                    }
                    _ => {}
                }
                part.push(ch);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| out.extend(addresses(i))),
        Value::Object(m) => out.extend(m.get("email").or_else(|| m.get("address")).map(addresses).unwrap_or_default()),
        _ => {}
    }
    out
}

/// The email addresses written in free text, lowercased. Each is a whole
/// run of the characters an address can hold (RFC 5322 atext, `.` and `@`),
/// so none is ever a piece cut from a longer one: `o'connor@corp.example`
/// and `!alerts@corp.example` stay what they are (review of #213). Only
/// what follows the domain is dropped (`bob@x.example).` reads as
/// `bob@x.example`); the domain must be letters, digits, `-` and dots.
/// Anything else reads as no address, and the send asks.
pub fn addresses_in(text: &str) -> Vec<String> {
    let in_address = |c: char| c.is_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.@".contains(c);
    text.split(|c: char| !in_address(c))
        .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| match w.split_once('@') {
            Some((name, domain)) => !name.is_empty() && domain.contains('.') && !domain.starts_with('.') && domain.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.'),
            None => false,
        })
        .collect()
}

/// One address from `Name <a@x>` or a bare `a@x`.
fn address(part: &str) -> Option<String> {
    let inner = match (part.rfind('<'), part.rfind('>')) {
        (Some(l), Some(r)) if l < r => &part[l + 1..r],
        _ => part,
    };
    let a = inner.trim().trim_matches('"').trim().to_lowercase();
    (!a.is_empty()).then_some(a)
}

#[cfg(test)]
mod address_tests {
    use super::{addresses, addresses_in};
    use serde_json::json;

    #[test]
    fn addresses_are_read_from_what_a_person_wrote() {
        let said = "Send Bob (Bob@Corp.example) the Q3 draft, cc <ann@x.example>; not @handle, me@, a@b or a@b. or \"quoted@q.example\".";
        assert_eq!(addresses_in(said), ["bob@corp.example", "ann@x.example", "quoted@q.example"]);
        assert!(addresses_in("Reply to Bob about Friday.").is_empty());
    }

    /// An address is never read as a piece of a longer one: the person who
    /// named `o'connor@` did not name `connor@` (review of #213).
    #[test]
    fn an_address_is_read_whole_never_as_a_piece_of_another() {
        assert_eq!(addresses_in("Send it to o'connor@corp.example."), ["o'connor@corp.example"]);
        assert_eq!(addresses_in("cc !alerts@corp.example!"), ["!alerts@corp.example"]);
        assert_eq!(addresses_in("to a+tag@corp.example, b.c@corp.example?"), ["a+tag@corp.example", "b.c@corp.example"]);
        for said in ["o'connor@corp.example", "!alerts@corp.example", "x.connor@corp.example", "'connor@corp.example"] {
            assert_eq!(addresses_in(said), [said], "a piece of {said} was read as an address");
        }
        assert!(addresses_in("\"john doe\"@corp.example and a@b'c.example").is_empty());
    }

    #[test]
    fn a_recipients_field_reads_as_exact_lowercased_addresses() {
        let h = json!(r#"Ann <Ann@X.example>, "Doe, Bo" <bo@y.example>,  team@lists.example ; "#);
        assert_eq!(addresses(&h), ["ann@x.example", "bo@y.example", "team@lists.example"]);
        assert_eq!(addresses(&json!(["A@b.example", { "email": "c@d.example" }, null])), ["a@b.example", "c@d.example"]);
        assert!(addresses(&json!("")).is_empty() && addresses(&json!(null)).is_empty());
    }
}
