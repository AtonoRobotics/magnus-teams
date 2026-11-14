//! The tools a chat turn may call besides reads, in the order
//! `tools::chat_specs` offers them: show an answer, start a task, propose a
//! schedule the person confirms in the thread, and control their tasks and
//! schedules; then `find_tools`.
use super::run_tools::component;
use super::{Class, Decl, Describe, Exec, Offer, Taint};
use crate::tools::{obj, Audience, Label, Risk};
use crate::tools_core::Op;
use serde_json::{json, Value};

const CHAT: Offer = Offer::Chat;
const CHAT_READ: Class = Class::new("chat", Risk::Read, Label::PRINCIPAL, Audience::Private);
const CHAT_WRITE: Class = Class::new("chat", Risk::ReversibleWrite, Label::PRINCIPAL, Audience::Private);

fn id() -> Value {
    obj(json!({ "id": { "type": "string" } }), &["id"])
}

fn suggest(d: &Describe) -> Value {
    let evidence = json!({ "type": "array", "minItems": 1, "description": d("suggest.evidence"), "items": { "type": "object", "properties": {
        "call": { "type": "string" }, "task": { "type": "string" }, "shows": { "type": "string" }
    }, "required": ["shows"] } });
    obj(
        json!({
            "title": { "type": "string" },
            "goal": { "type": "string" },
            "evidence": evidence,
            "permissions": { "type": "array", "minItems": 1, "items": { "type": "string" } },
            "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" },
            "again": { "type": "string" }, "changed": { "type": "string" }
        }),
        &["title", "goal", "evidence", "permissions"],
    )
}

pub(super) static DECLS: &[Decl] = &[
    Decl::record("show", CHAT, |_| obj(json!({ "components": { "type": "array", "minItems": 1, "items": component(crate::compose::SHOWN) } }), &["components"])),
    // Start a task, or set up a schedule, at the person's request (spec rev
    // 140: asking grants the work). Both carry the turn's taint. No budget is
    // the model's to set: a task's comes from the server (spec rev 133).
    Decl::tool("create_task", CHAT_WRITE, Exec::Core(Op::CreateTask), CHAT, |_| {
        obj(json!({ "goal": { "type": "string" }, "title": { "type": "string" }, "grant": { "type": "string" } }), &["goal", "title"])
    }),
    Decl::tool("create_schedule", CHAT_READ, Exec::Core(Op::CreateSchedule), CHAT, |_| {
        obj(
            json!({ "title": { "type": "string" }, "goal": { "type": "string" }, "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" } }),
            &["title", "goal"],
        )
    }),
    // Magnus's own idea, from chat, a task or a review: a card the person
    // accepts with one action (a task or a schedule starts), puts off or
    // declines. Nothing happens until they accept.
    Decl::tool("suggest", CHAT_READ, Exec::Core(Op::Suggest), CHAT, suggest),
    // A grant card, when the person asked for standing authority over a
    // kind of their work (#235): nothing is granted until they accept it.
    // Revoking one of theirs at their word only narrows what Magnus may do.
    Decl::tool("revoke_grant", CHAT_WRITE, Exec::Core(Op::RevokeGrant), CHAT, |_| obj(json!({ "grant": { "type": "string" } }), &["grant"])),
    Decl::tool("bundle_share", Class::new("bundle", Risk::ReversibleWrite, Label::PRINCIPAL, Audience::Private), Exec::Core(Op::Bundle), CHAT, |_| {
        obj(json!({ "path": { "type": "string" }, "groups": { "type": "array", "items": { "type": "string" } }, "everyone": { "type": "boolean" } }), &["path"])
    }),
    // Control of the person's own tasks and schedules, as the app's buttons
    // (behavior spec, Tasks and schedules in conversation). What they list
    // carries what their runs read.
    Decl::tool("list_tasks", CHAT_READ, Exec::Core(Op::Control), CHAT, |_| obj(json!({ "limit": { "type": "integer" } }), &[])).taint(Taint::Listed),
    Decl::tool("list_schedules", CHAT_READ, Exec::Core(Op::Control), CHAT, |_| obj(json!({}), &[])).taint(Taint::Listed),
    Decl::tool("get_task", CHAT_READ, Exec::Core(Op::Control), CHAT, |_| id()).taint(Taint::Listed),
    Decl::tool("get_schedule", CHAT_READ, Exec::Core(Op::Control), CHAT, |_| id()).taint(Taint::Listed),
    Decl::tool("cancel_task", CHAT_WRITE, Exec::Core(Op::Control), CHAT, |_| id()),
    Decl::tool("pause_schedule", CHAT_WRITE, Exec::Core(Op::Control), CHAT, |_| id()),
    Decl::tool("resume_schedule", CHAT_WRITE, Exec::Core(Op::Control), CHAT, |_| id()),
    Decl::tool("edit_schedule", CHAT_WRITE, Exec::Core(Op::Control), CHAT, |_| {
        obj(
            json!({ "id": { "type": "string" }, "every_minutes": { "type": "integer" }, "daily_at": { "type": "string" }, "until": { "type": "string" }, "max_runs": { "type": "integer" } }),
            &["id"],
        )
    }),
    Decl::tool("cancel_schedule", CHAT_WRITE, Exec::Core(Op::Control), CHAT, |_| id()),
    // Past `LOAD_ALL` tools across the person's connected apps, a run is
    // offered only those it found with this (connectors spec: tools searched
    // across connectors and full schemas loaded on demand).
    Decl::tool(
        "find_tools",
        Class::new("apps", Risk::Read, Label::PRINCIPAL, Audience::Private),
        Exec::Core(Op::FindTools),
        Offer::FindTools,
        |_| json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
    )
    .in_team(),
];
