//! Content-field registry for events stored in `public.content`.
//! Extracted from `events.rs` to keep the event enum module below its size ratchet.

use super::OLD_LANE_KINDS;

/// What each kind of event holds that a person or a source wrote (stored in `public.content`).
pub const CONTENT: &[(&str, &[&str])] = &[
    ("task_created", &["text", "attachments", "title", "chat.conversation", "project.name", "project.instructions", "scope"]),
    ("file_uploaded", &["name", "data_b64"]),
    ("project_set", &["name", "instructions"]),
    ("thread_branched", &["entries", "title"]),
    ("schedule_set", &["text", "title", "scope"]),
    ("model_reply", &["blocks", "state"]),
    ("verifier_reply", &["blocks"]),
    ("memory_pass", &["blocks"]),
    ("intent", &["input"]),
    ("tool_result", &["text", "data"]),
    ("handback", &["image_b64"]),
    ("contract", &["contract"]),
    ("plan", &["plan"]),
    ("verification", &["criterion", "evidence"]),
    ("evidence", &["found", "sources", "note", "to"]),
    ("completed", &["summary", "view", "spoken"]),
    ("failed", &["reason"]),
    ("item", &["summary", "details"]),
    ("item_resolved", &["text"]),
    ("output", &["name", "data_b64"]),
    ("playbook", &["text", "addresses"]),
    ("verdict", &["note"]),
    ("graded", &["details"]),
    ("memory", &["fact", "topic"]),
    ("knowledge_added", &["title", "text", "source", "verification", "value"]),
    ("bundle_written", &["frontmatter", "body"]),
    ("knowledge_extraction", &["reply"]),
    ("spoken_written", &["script"]),
    ("disagreement_recorded", &["position", "decision"]),
    ("disagreement_outcome", &["outcome"]),
    ("card_declined", &["reason"]),
    ("schedule_ended", &["reason"]),
    ("draft_scoped", &["participants"]),
    ("grant_set", &["title"]),
    ("commitment_recorded", &["what", "to", "quote"]),
    ("commitment_changed", &["what", "to", "note"]),
    ("watched_seen", &["ids"]),
    ("rule", &["words"]),
    ("spend", &["reason", "note"]),
    ("team_mission", &["goal", "objective", "draft", "contribution", "markdown", "held_because"]),
    ("team", &["name", "message", "history", "file", "text", "note", "def", "entry", "routine", "chat", "coedit"]),
    ("prefetch_recorded", &["pointers"]),
    ("outside_agent", &["text", "reply", "draft", "held_because"]),
    ("staged", &["sentence", "input", "shown", "provenance"]),
];

/// The content fields of one kind of event (a lane stored under its lane's kind, a mission's); none for most.
pub fn content_fields(kind: &str) -> &'static [&'static str] {
    CONTENT.iter().find(|(k, _)| *k == kind || (*k == "team_mission" && OLD_LANE_KINDS.contains(&kind))).map(|(_, f)| *f).unwrap_or(&[])
}
