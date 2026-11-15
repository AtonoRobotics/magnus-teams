//! A run's built-in tools and records, in the order `tools::builtin_specs`
//! offers them.
use super::{Class, Decl, Describe, Exec, Need, Offer, Taint, WebRead};
use crate::tools::{obj, Audience, Label, Risk};
use crate::tools_core::Op;
use serde_json::{json, Value};

const ALWAYS: Offer = Offer::Run(Need::Always);
const SEARCH: Offer = Offer::Run(Need::Search);
const SANDBOX: Offer = Offer::Run(Need::Sandbox);
const BROWSER: Offer = Offer::Run(Need::Browser);
const COMPUTER: Offer = Offer::Run(Need::Computer);

/// The person's own: it reaches no one and carries nothing in.
const fn mine(app: &'static str, risk: Risk) -> Class {
    Class::new(app, risk, Label::PRINCIPAL, Audience::Private)
}
const IN_SANDBOX: Class = mine("sandbox", Risk::Sandbox);
const WEB: Class = Class::new("web", Risk::Read, Label::UNTRUSTED, Audience::Public);
const BROWSE: Class = Class::new("browser", Risk::Read, Label::UNTRUSTED, Audience::Public);
const BROWSE_ACT: Class = Class::new("browser", Risk::ReversibleWrite, Label::UNTRUSTED, Audience::Public);
/// Through a person's own sign-in: what it reads is theirs, and written by others.
const SIGNED_IN: Label = Label { untrusted: true, private: true };
const AS_READ: Class = Class::new("browser", Risk::Read, SIGNED_IN, Audience::Private);
const AS_ACT: Class = Class::new("browser", Risk::ReversibleWrite, SIGNED_IN, Audience::Private);
const MEMORY: Class = mine("memory", Risk::ReversibleWrite);
const APPS: Class = mine("apps", Risk::Read);

const COMPUTER_WORDS: &str = "Working on its computer";
const PAGE_WORDS: &str = "Reading a page";
const BROWSING: &str = "Browsing";

fn target() -> Value {
    json!({ "type": "string", "description": "an element ref like e12 from the latest snapshot, or visible text" })
}

/// A tool acting as a signed-in identity: its id first, then its own fields.
fn as_identity(extra: Value, needs: &[&str]) -> Value {
    let mut props = json!({ "identity": { "type": "string", "description": "the identity's id" } });
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    let mut required = vec!["identity"];
    required.extend(needs);
    obj(props, &required)
}

fn xy() -> Value {
    json!({ "type": "integer" })
}

/// One component of a composition, of the given kinds. Every field is
/// declared: a model that keeps to the schema cannot otherwise fill one in.
pub(super) fn component(kinds: &[&str]) -> Value {
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

fn task_complete(d: &Describe) -> Value {
    obj(
        json!({
            "summary": { "type": "string", "description": d("task_complete.summary") },
            // Every component field is declared: a model that keeps to
            // the schema cannot otherwise fill one in.
            "deliverable": { "type": "array", "minItems": 1, "description": d("task_complete.deliverable"), "items": component(crate::compose::KINDS) },
            "spoken": { "type": "string", "description": d("task_complete.spoken") },
            "sources": { "type": "array", "description": d("task_complete.sources"), "items": obj(json!({ "claim": { "type": "string" }, "call": { "type": "string" } }), &["claim", "call"]) }
        }),
        &["deliverable", "summary"],
    )
}

fn knowledge_add(d: &Describe) -> Value {
    obj(
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
    )
}

pub(super) static DECLS: &[Decl] = &[
    Decl::tool("find_apps", APPS, Exec::Core(Op::FindApps), ALWAYS, |_| obj(json!({ "query": { "type": "string" } }), &[])),
    Decl::tool("app_details", APPS, Exec::Core(Op::AppDetails), ALWAYS, |_| obj(json!({ "name": { "type": "string" } }), &["name"])),
    Decl::tool("offer_app", APPS, Exec::Core(Op::OfferApp), ALWAYS, |_| obj(json!({ "name": { "type": "string" }, "why": { "type": "string" } }), &["name", "why"])),
    Decl::record("task_plan", ALWAYS, |_| {
        obj(
            json!({ "title": { "type": "string" }, "units": { "type": "array", "minItems": 1, "items": { "type": "object", "properties": {
                "id": { "type": "string" }, "goal": { "type": "string" }, "done_when": { "type": "string" },
                "status": { "type": "string", "enum": ["pending", "active", "done", "dropped"] }
            }, "required": ["id", "goal"] } } }),
            &["units"],
        )
    }),
    Decl::record("task_complete", ALWAYS, task_complete).doing("Finishing up"),
    Decl::record("task_fail", ALWAYS, |_| obj(json!({ "reason": { "type": "string" } }), &["reason"])),
    Decl::record("ask_principal", ALWAYS, |_| obj(json!({ "question": { "type": "string" }, "options": { "type": "array", "items": { "type": "string" } } }), &["question"])),
    Decl::record("propose_policy", ALWAYS, |_| obj(json!({ "layer": { "type": "string" }, "cedar": { "type": "string" }, "rationale": { "type": "string" } }), &["layer", "cedar", "rationale"])),
    Decl::record("end_schedule", ALWAYS, |_| obj(json!({ "reason": { "type": "string" } }), &["reason"])),
    Decl::record("wait_until", ALWAYS, |_| obj(json!({ "at": { "type": "string" }, "reason": { "type": "string" } }), &["at", "reason"])),
    Decl::record("notify", ALWAYS, |_| obj(json!({ "message": { "type": "string" } }), &["message"])),
    Decl::record("observe", ALWAYS, |_| obj(json!({ "what": { "type": "string" }, "why": { "type": "string" }, "evidence": { "type": "string" } }), &["what", "why", "evidence"])),
    Decl::record("interrupt", ALWAYS, |_| obj(json!({ "what": { "type": "string" }, "why": { "type": "string" } }), &["what", "why"])),
    Decl::tool("web_fetch", WEB, Exec::Core(Op::WebFetch), ALWAYS, |_| obj(json!({ "url": { "type": "string" } }), &["url"])).doing(PAGE_WORDS).web(WebRead::OpensUrl).in_team(),
    Decl::tool("memory_write", MEMORY, Exec::Core(Op::MemoryWrite), ALWAYS, |_| obj(json!({ "fact": { "type": "string" }, "topic": { "type": "string" } }), &["fact"])),
    // The person's relationship memory (behavior spec, Per-user
    // relationship): their own notes, on the server, versioned.
    Decl::tool("relationship_note", MEMORY, Exec::Core(Op::Relationship), ALWAYS, |_| {
        obj(json!({ "action": { "type": "string", "enum": ["add", "change", "remove"] }, "note": { "type": "string" }, "text": { "type": "string" } }), &["action"])
    })
    .person_only(),
    // The person's disagreement log (behavior spec): theirs, on the server.
    Decl::tool("disagreement", MEMORY, Exec::Core(Op::Disagreement), ALWAYS, |_| {
        obj(
            json!({ "action": { "type": "string", "enum": ["record", "outcome"] }, "position": { "type": "string" }, "decision": { "type": "string" }, "id": { "type": "string" }, "outcome": { "type": "string" } }),
            &["action"],
        )
    })
    .person_only(),
    // The person's commitment ledger (#236): what they and Magnus said
    // they would do, recorded from their words or what a run read; theirs.
    Decl::tool("record_commitment", MEMORY, Exec::Core(Op::RecordCommitment), ALWAYS, |_| {
        obj(
            json!({
                "who": { "type": "string", "enum": ["person", "magnus"] }, "what": { "type": "string" }, "to": { "type": "string" },
                "by": { "type": "string" }, "quote": { "type": "string" }, "call": { "type": "string" }
            }),
            &["who", "what", "quote"],
        )
    }),
    Decl::tool("update_commitment", MEMORY, Exec::Core(Op::UpdateCommitment), ALWAYS, |_| {
        obj(json!({ "id": { "type": "string" }, "status": { "type": "string", "enum": ["done", "released"] }, "note": { "type": "string" } }), &["id", "status"])
    }),
    // The org context bundle (org context build spec): reads carry the
    // taint their documents were written under; writes stay on the
    // server, versioned and reversible.
    Decl::tool("bundle_read", mine("bundle", Risk::Read), Exec::Core(Op::Bundle), ALWAYS, |_| obj(json!({ "path": { "type": "string" } }), &["path"])).taint(Taint::Returned),
    Decl::tool("bundle_search", mine("bundle", Risk::Read), Exec::Core(Op::Bundle), ALWAYS, |_| {
        obj(json!({ "query": { "type": "string" }, "kinds": { "type": "array", "items": { "type": "string" } }, "limit": { "type": "integer" } }), &["query"])
    })
    .taint(Taint::Returned),
    Decl::tool("bundle_write", mine("bundle", Risk::ReversibleWrite), Exec::Core(Op::Bundle), ALWAYS, |_| {
        obj(json!({ "path": { "type": "string" }, "frontmatter": { "type": "object" }, "body": { "type": "string" } }), &["path", "frontmatter", "body"])
    }),
    Decl::tool("memory_search", mine("memory", Risk::Read), Exec::Core(Op::MemorySearch), ALWAYS, |_| obj(json!({ "query": { "type": "string" } }), &["query"])).taint(Taint::Memory),
    // The company knowledge base. A search's result carries the taint of
    // what it returns; filing research changes nothing outside the server.
    Decl::tool("knowledge_search", mine("knowledge", Risk::Read), Exec::Core(Op::KnowledgeSearch), ALWAYS, |_| obj(json!({ "query": { "type": "string" } }), &["query"])).taint(Taint::Returned),
    Decl::tool("knowledge_add", mine("knowledge", Risk::ReversibleWrite), Exec::Core(Op::KnowledgeAdd), ALWAYS, knowledge_add),
    // A project's reference file, read when the work needs it (M2). Its
    // result carries the file's own taint; an upload is private and untrusted.
    Decl::tool("project_read", mine("library", Risk::Read), Exec::Core(Op::ProjectRead), ALWAYS, |_| {
        obj(json!({ "name": { "type": "string", "description": "the file's name, as the project lists it" } }), &["name"])
    })
    .taint(Taint::ProjectFile),
    Decl::tool("playbook_update", mine("playbook", Risk::ReversibleWrite), Exec::Core(Op::Playbook), ALWAYS, |_| {
        obj(
            json!({
                "op": { "type": "string", "enum": ["add", "edit", "retire"] },
                "item": { "type": "string", "description": "the item id, for edit and retire" },
                "text": { "type": "string" },
                "addresses": { "type": "string", "description": "the failure this item addresses" }
            }),
            &["op"],
        )
    }),
    Decl::tool("web_search", WEB, Exec::Core(Op::WebSearch), SEARCH, |_| obj(json!({ "query": { "type": "string" } }), &["query"])).doing("Searching the web").web(WebRead::Query).in_team(),
    Decl::tool("open_attachment", Class::new("sandbox", Risk::Sandbox, Label::UNTRUSTED, Audience::Private), Exec::Core(Op::Side), SANDBOX, |_| {
        obj(json!({ "name": { "type": "string" }, "command": { "type": "string", "description": "optional: a command to run on the file, which is at /workspace/inputs/attachment" } }), &["name"])
    }),
    Decl::tool("import_attachment", Class::new("sandbox", Risk::Sandbox, Label::UNTRUSTED, Audience::Private), Exec::Core(Op::Side), SANDBOX, |_| {
        obj(json!({ "name": { "type": "string" } }), &["name"])
    }),
    Decl::tool("start_sub_agent", IN_SANDBOX, Exec::Core(Op::Side), ALWAYS, |_| {
        obj(
            // No grant: a sub-agent works under its parent's (owner, 2026-11-10).
            json!({ "goal": { "type": "string" }, "tools": { "type": "array", "items": { "type": "string" } }, "budget_share": { "type": "number" } }),
            &["goal"],
        )
    })
    .taint(Taint::Returned)
    .in_team(),
    Decl::tool("browser_navigate", BROWSE, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({ "url": { "type": "string" } }), &["url"])).doing(PAGE_WORDS).web(WebRead::OpensUrl).in_team(),
    Decl::tool("browser_snapshot", BROWSE, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({}), &[])).doing(PAGE_WORDS).web(WebRead::OpenPage).in_team(),
    Decl::tool("browser_click", BROWSE_ACT, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({ "target": target() }), &["target"])).doing(BROWSING).in_team(),
    Decl::tool("browser_type", BROWSE_ACT, Exec::Core(Op::Browser), BROWSER, |_| {
        obj(json!({ "target": target(), "text": { "type": "string" }, "submit": { "type": "boolean" } }), &["target", "text"])
    })
    .doing(BROWSING)
    .in_team(),
    Decl::tool("browser_select", BROWSE_ACT, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({ "target": target(), "value": { "type": "string" } }), &["target", "value"])).doing(BROWSING).in_team(),
    Decl::tool("browser_back", BROWSE, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({}), &[])).doing(BROWSING).web(WebRead::OpenPage).in_team(),
    Decl::tool("browser_screenshot", BROWSE, Exec::Core(Op::Browser), BROWSER, |_| obj(json!({}), &[])).doing(BROWSING).web(WebRead::Picture).in_team(),
    // As a person's signed-in identity: never a team thread's.
    Decl::tool("browser_as_navigate", AS_READ, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({ "url": { "type": "string" } }), &["url"])).doing(BROWSING),
    Decl::tool("browser_as_snapshot", AS_READ, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({}), &[])).doing(BROWSING),
    Decl::tool("browser_as_click", AS_ACT, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({ "target": target() }), &["target"])).doing(BROWSING),
    Decl::tool("browser_as_type", AS_ACT, Exec::Core(Op::Browser), COMPUTER, |_| {
        as_identity(json!({ "target": target(), "text": { "type": "string" }, "submit": { "type": "boolean" } }), &["target", "text"])
    })
    .doing(BROWSING),
    Decl::tool("browser_as_select", AS_ACT, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({ "target": target(), "value": { "type": "string" } }), &["target", "value"])).doing(BROWSING),
    Decl::tool("browser_as_back", AS_READ, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({}), &[])).doing(BROWSING),
    Decl::tool("browser_as_screenshot", AS_READ, Exec::Core(Op::Browser), COMPUTER, |_| as_identity(json!({}), &[])).doing(BROWSING),
    Decl::tool("browser_sign_in", mine("browser", Risk::ReversibleWrite), Exec::Core(Op::Browser), COMPUTER, |_| {
        obj(json!({ "identity": { "type": "string", "description": "the identity's id" }, "username_field": target(), "password_field": target() }), &["identity", "password_field"])
    })
    .doing("Signing in"),
    Decl::tool(crate::computer::REQUEST_TOOL, mine("computer", Risk::Read), Exec::Core(Op::Computer), COMPUTER, crate::computer::request_schema).doing(COMPUTER_WORDS).in_team(),
    Decl::tool(crate::computer::EXEC_TOOL, mine("computer", Risk::Sandbox), Exec::Core(Op::Computer), COMPUTER, crate::computer::exec_schema).doing(COMPUTER_WORDS).in_team(),
    Decl::tool(crate::computer::INSTALL_TOOL, mine("computer", Risk::ReversibleWrite), Exec::Core(Op::Computer), COMPUTER, crate::computer::install_schema).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("add_computer_destination", Class::new("computer", Risk::ReversibleWrite, Label::PRINCIPAL, Audience::Public), Exec::Core(Op::Side), COMPUTER, |_| {
        obj(json!({ "host": { "type": "string" } }), &["host"])
    }),
    Decl::tool("computer_screenshot", IN_SANDBOX, Exec::Core(Op::Computer), SANDBOX, |_| obj(json!({}), &[])).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("computer_click", IN_SANDBOX, Exec::Core(Op::Computer), SANDBOX, |_| {
        obj(json!({ "x": xy(), "y": xy(), "button": { "type": "string", "enum": ["left", "right", "middle"] }, "double": { "type": "boolean" } }), &["x", "y"])
    })
    .doing(COMPUTER_WORDS)
    .in_team(),
    Decl::tool("computer_type", IN_SANDBOX, Exec::Core(Op::Computer), SANDBOX, |_| obj(json!({ "text": { "type": "string" } }), &["text"])).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("computer_key", IN_SANDBOX, Exec::Core(Op::Computer), SANDBOX, |_| obj(json!({ "keys": { "type": "string" } }), &["keys"])).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("computer_scroll", IN_SANDBOX, Exec::Core(Op::Computer), SANDBOX, |_| obj(json!({ "x": xy(), "y": xy(), "down": { "type": "integer" } }), &["x", "y", "down"])).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("deliver_file", IN_SANDBOX, Exec::Core(Op::DeliverFile), SANDBOX, |_| {
        obj(json!({ "path": { "type": "string" }, "name": { "type": "string" }, "from": { "type": "string", "enum": ["computer", "sandbox"] } }), &["path"])
    })
    .in_team(),
    Decl::tool("sandbox_exec", IN_SANDBOX, Exec::Core(Op::SandboxExec), SANDBOX, |_| obj(json!({ "command": { "type": "string" }, "timeout_s": { "type": "integer" } }), &["command"])).doing(COMPUTER_WORDS).in_team(),
    Decl::tool("sandbox_write_file", IN_SANDBOX, Exec::Core(Op::SandboxWrite), SANDBOX, |_| {
        obj(json!({ "path": { "type": "string" }, "content": { "type": "string" }, "base64": { "type": "boolean" } }), &["path", "content"])
    })
    .doing(COMPUTER_WORDS)
    .in_team(),
    Decl::tool("sandbox_read_file", IN_SANDBOX, Exec::Core(Op::SandboxRead), SANDBOX, |_| obj(json!({ "path": { "type": "string" } }), &["path"])).doing(COMPUTER_WORDS).in_team(),
];
