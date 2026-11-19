//! A team workspace's own tools, in the order `team::specs` offers them.
//! The run tools a team thread is offered too are declared with the run's
//! (`in_team`); these serve only a team thread, but for reading a base,
//! which a person's own agent may do for a base its owner may read.
use super::{Class, Decl, Exec, Offer, Taint};
use crate::team::TeamOp;
use crate::tools::{obj, Audience, Label, Risk};
use serde_json::json;

const TEAM: Offer = Offer::Team;
const TO_TEAM: Class = Class::new("team", Risk::ReversibleWrite, Label::PRINCIPAL, Audience::Internal);

pub(super) static DECLS: &[Decl] = &[
    Decl::tool("team_read_file", Class::new("team", Risk::Read, Label::UNTRUSTED, Audience::Internal), Exec::Team(TeamOp::ReadFile), TEAM, |_| {
        obj(json!({ "name": { "type": "string" } }), &["name"])
    })
    .doing("Reading a team file")
    .in_team(),
    Decl::tool("team_write_file", Class::new("team", Risk::ReversibleWrite, Label::PRINCIPAL, Audience::Private), Exec::Team(TeamOp::WriteFile), TEAM, |_| {
        obj(json!({ "name": { "type": "string" }, "content": { "type": "string" } }), &["name", "content"])
    })
    .doing("Writing a file for the team")
    .in_team(),
    Decl::tool("team_post", TO_TEAM, Exec::Team(TeamOp::Post), TEAM, |_| obj(json!({ "text": { "type": "string" } }), &["text"])).doing("Posting an update").in_team(),
    Decl::tool("team_add_fact", TO_TEAM, Exec::Team(TeamOp::AddFact), TEAM, |_| {
        obj(json!({ "text": { "type": "string" }, "kind": { "type": "string" }, "source": { "type": "string" }, "workspace_level": { "type": "boolean" } }), &["text"])
    })
    .doing("Updating the knowledge base")
    .in_team(),
    Decl::tool("team_fix_entry", TO_TEAM, Exec::Team(TeamOp::FixEntry), TEAM, |_| {
        obj(json!({ "id": { "type": "string" }, "statement": { "type": "string" }, "retire": { "type": "boolean" }, "superseded_by": { "type": "string" } }), &["id"])
    })
    .doing("Updating the knowledge base")
    .in_team(),
    Decl::tool("team_create", TO_TEAM, Exec::Team(TeamOp::Create), TEAM, |_| {
        obj(json!({ "kind": { "type": "string", "enum": ["project", "channel"] }, "name": { "type": "string" } }), &["kind", "name"])
    })
    .doing("Making a space for the team")
    .in_team(),
    Decl::tool("team_note", TO_TEAM, Exec::Team(TeamOp::Note), TEAM, |_| obj(json!({ "text": { "type": "string" } }), &["text"])).doing("Taking notes").in_team(),
    Decl::tool(
        "team_ask_personal_agent",
        Class::new("team", Risk::ReversibleWrite, Label { untrusted: true, private: true }, Audience::Internal),
        Exec::Team(TeamOp::AskPersonalAgent),
        TEAM,
        |_| obj(json!({ "request": { "type": "string" }, "app": { "type": "string" }, "member": { "type": "string" } }), &["request"]),
    )
    .taint(Taint::Returned)
    .doing("Asking personal agent")
    .in_team(),
    Decl::tool(
        "team_read_knowledge",
        Class::new("team", Risk::Read, Label { untrusted: true, private: true }, Audience::Internal),
        Exec::Team(TeamOp::ReadKnowledge),
        TEAM,
        |_| obj(json!({ "workspace": { "type": "string" }, "project": { "type": "string" } }), &[]),
    )
    .doing("Reading the knowledge base")
    .in_team(),
];
