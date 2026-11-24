//! A team workspace's own tools, in the order `team::specs` offers them.
//! The run tools a team thread is offered too are declared with the run's
//! (`in_team`). The team agent's own tools went with its runs
//! (2026-11-22-retire-team-agent-runs); what is left is reading a base,
//! which a person's own agent may do for a base its owner may read, and a
//! shared project's files: reading one, and proposing an edit to a
//! co-edited one, which is its person's own draft until its lane's result
//! is released (`team::coedit`), so it changes nothing anyone sees.
use super::{Class, Decl, Exec, Offer, Taint};
use crate::team::TeamOp;
use crate::tools::{obj, Audience, Label, Risk};
use serde_json::json;

pub(super) static DECLS: &[Decl] = &[
    Decl::tool(
        "team_read_knowledge",
        Class::new("team", Risk::Read, Label { untrusted: true, private: true }, Audience::Internal),
        Exec::Team(TeamOp::ReadKnowledge),
        Offer::Team,
        |_| obj(json!({ "workspace": { "type": "string" }, "project": { "type": "string" } }), &[]),
    )
    .taint(Taint::Returned)
    .doing("Reading the knowledge base")
    .in_team(),
    Decl::tool("team_read_file", Class::new("team", Risk::Read, Label { untrusted: true, private: true }, Audience::Internal), Exec::Team(TeamOp::ReadFile), Offer::Team, |_| {
        obj(json!({ "workspace": { "type": "string" }, "name": { "type": "string" } }), &["workspace", "name"])
    })
    .taint(Taint::Returned)
    .doing("Reading a shared file"),
    Decl::tool(
        "team_propose_edit",
        Class::new("team", Risk::Read, Label { untrusted: true, private: true }, Audience::Internal),
        Exec::Team(TeamOp::ProposeEdit),
        Offer::Team,
        |_| {
            obj(
                json!({ "workspace": { "type": "string" }, "name": { "type": "string" }, "base": { "type": "integer" }, "content": { "type": "string" } }),
                &["workspace", "name", "base", "content"],
            )
        },
    )
    .taint(Taint::Returned)
    .doing("Proposing an edit to a shared file"),
];
