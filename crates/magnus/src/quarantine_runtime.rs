//! Runtime wiring for the CaMeL-style Quarantined Reader (W1.1, B1, B2, M4, N12, P6).
//!
//! - Routine-scoped envelopes count only writable external destinations (excluding
//!   read-only `Host` URLs) when deciding `routine_needs_quarantine`.
//! - Whether a run is quarantined is recorded with the run itself
//!   (`RunConfig::quarantine`, set when its schedule fires, in the same write
//!   that creates it), so it holds from its first step and through a restart
//!   or a replay. Every descendant (`requester_run` lineage: sub-agents,
//!   `OpenWeb`) inherits it.
//! - Untrusted free-text fields are kept behind opaque handles (`$q1`, `$q2`,
//!   ...) so the planner can template them into outbound message bodies
//!   without ever reading raw untrusted prose. A handle's text is recorded in
//!   the data of the tool result that made it ([`HANDLES_KEY`]), which the
//!   model is never shown.
//! - Opaque handles are refused in destination fields. In an action that
//!   changes something and reaches others they become their text at the boundary
//!   ([`Kernel::expand_handles`]), before the send-path `outgoing_finding`
//!   and action review, which judge the expanded body.

use crate::boundary::Action;
use crate::envelope::{Destination, Envelope};
use crate::kernel::Kernel;
use crate::state::{Run, Schedule, State};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const MAX_HANDLE_TEXT_BYTES: usize = 4096;

/// The key, in a quarantined tool result's data, of the handles it made:
/// `{"$q1": "<raw untrusted text>"}`. Recorded with the result, they last
/// as long as it does and are erased with it.
pub const HANDLES_KEY: &str = "quarantine_handles";

/// Fields that say where an action goes: a handle in one is refused (N12).
const DESTINATION_FIELDS: &[&str] = &[
    "to",
    "cc",
    "bcc",
    "recipient",
    "recipients",
    "reply_to",
    "from",
    "url",
    "uri",
    "link",
    "host",
    "domain",
    "channel",
    "channel_id",
    "address",
    "email",
    "phone",
    "webhook",
    "endpoint",
];

/// Default typed-only extraction schema (zero planner-visible free text, B1).
pub fn default_typed_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "relevant": { "type": "boolean" },
            "priority": { "type": "string", "enum": ["low", "medium", "high"] },
            "count": { "type": "integer" }
        }
    })
}

/// Validates a routine extraction schema: external-reaching routines may not
/// declare planner-visible free-text (`string` without `enum`) fields (B1).
pub fn validate_routine_schema(schema: &Value, owner_only: bool) -> Result<(), String> {
    if owner_only {
        return Ok(());
    }
    let Some(props) = schema["properties"].as_object() else {
        return Err("quarantine schema must declare an object with properties".into());
    };
    for (name, prop) in props {
        let ty = prop["type"].as_str().unwrap_or_default();
        if ty == "string" {
            let has_enum = prop["enum"].as_array().is_some_and(|a| !a.is_empty());
            let is_opaque = prop["x-opaque-handle"].as_bool() == Some(true);
            if !has_enum && !is_opaque {
                return Err(format!(
                    "routine schema field '{name}' declares planner-visible free text, which is forbidden on external-reaching routines (use enum or x-opaque-handle: true)"
                ));
            }
        }
    }
    Ok(())
}

/// Returns `true` if `envelope` contains any **writable** destination reaching
/// someone other than `owner_email` (ignoring read-only `Host` entries, M4).
pub fn envelope_reaches_others_writable(envelope: &Envelope, owner_email: Option<&str>) -> bool {
    let owner = owner_email.and_then(Destination::parse_email);
    envelope.allowed_destinations.iter().any(|dest| match (dest, &owner) {
        (Destination::Host { .. }, _) => false,
        (d, Some(own)) if d == own => false,
        _ => true,
    })
}

/// The opaque handles (`$q1`, `$q2`, ...) written in `value`, each whole.
fn handle_tokens(value: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = value;
    while let Some(at) = rest.find("$q") {
        let digits = rest[at + 2..].bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            out.push(&rest[at..at + 2 + digits]);
        }
        rest = &rest[at + 2 + digits..];
    }
    out
}

/// Returns `true` if `value` contains an opaque quarantine handle (`$q1`, `$q2`, ...).
/// Opaque handles are forbidden in destination fields (N12).
pub fn contains_opaque_handle(value: &str) -> bool {
    !handle_tokens(value).is_empty()
}

/// The handles one quarantined tool result makes, numbered after the ones
/// its run's tree already has, so each names one text.
#[derive(Debug, Default)]
pub struct Handles {
    next: usize,
    texts: Map<String, Value>,
}

impl Handles {
    /// Handles numbered after `existing` ones.
    pub fn after(existing: usize) -> Handles {
        Handles { next: existing + 1, texts: Map::new() }
    }

    /// Keep an untrusted free-text blob behind a new handle, and name it.
    pub fn store(&mut self, text: &str) -> String {
        let mut clipped = text.to_string();
        if clipped.len() > MAX_HANDLE_TEXT_BYTES {
            let mut cut = MAX_HANDLE_TEXT_BYTES;
            while !clipped.is_char_boundary(cut) {
                cut -= 1;
            }
            clipped.truncate(cut);
        }
        let handle = format!("$q{}", self.next);
        self.next += 1;
        self.texts.insert(handle.clone(), Value::String(clipped));
        handle
    }

    /// The tool result's data with these handles recorded in it.
    pub fn into_data(self, data: Option<Value>) -> Option<Value> {
        if self.texts.is_empty() {
            return data;
        }
        let mut data = match data {
            Some(Value::Object(m)) => m,
            Some(other) => Map::from_iter([("result".to_string(), other)]),
            None => Map::new(),
        };
        data.insert(HANDLES_KEY.into(), Value::Object(self.texts));
        Some(Value::Object(data))
    }
}

/// Every handle made in the run tree under `root` (the top run and every
/// sub-agent below it), read from their recorded results.
pub fn handles_of(s: &State, root: &str) -> BTreeMap<String, String> {
    let below = s.sub_agents.below(root).into_iter().map(|(_, a)| a.child);
    let mut out = BTreeMap::new();
    for run in std::iter::once(root.to_string()).chain(below).filter_map(|r| s.runs.get(&r)) {
        for made in run.results.values().filter_map(|r| r.data.as_ref().and_then(|d| d[HANDLES_KEY].as_object())) {
            out.extend(made.iter().filter_map(|(h, t)| Some((h.clone(), t.as_str()?.to_string()))));
        }
    }
    out
}

/// Expand the `$qN` opaque handles inside an outbound message body: each
/// whole handle becomes its text (`$q1` never matches the start of
/// `$q10`); one with no text stays as written. The result must still pass
/// through `outgoing_finding` and action review (P6).
pub fn expand_body_handles(handles: &BTreeMap<String, String>, body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    for token in handle_tokens(body) {
        let Some(at) = rest.find(token) else { continue };
        out.push_str(&rest[..at]);
        out.push_str(handles.get(token).map_or(token, String::as_str));
        rest = &rest[at + token.len()..];
    }
    out.push_str(rest);
    out
}

/// Sanitize a quarantined extraction JSON object: keep only declared typed
/// fields (`boolean`, `number`/`integer`, `enum`), and replace any declared
/// `x-opaque-handle: true` string field with a handle kept in `handles`.
pub fn sanitize_with_handles(handles: &mut Handles, raw: &Value, schema: &Value) -> Value {
    let Some(props) = schema["properties"].as_object() else {
        return json!({});
    };
    let Some(obj) = raw.as_object() else {
        return json!({});
    };
    let mut out = Map::new();
    for (k, prop_schema) in props {
        let Some(val) = obj.get(k) else { continue };
        let expected = prop_schema["type"].as_str().unwrap_or_default();
        match (expected, val) {
            ("boolean", Value::Bool(b)) => {
                out.insert(k.clone(), Value::Bool(*b));
            }
            ("number" | "integer", Value::Number(n)) => {
                out.insert(k.clone(), Value::Number(n.clone()));
            }
            ("string", Value::String(s)) => {
                if let Some(allowed) = prop_schema["enum"].as_array() {
                    if allowed.iter().any(|a| a.as_str() == Some(s.as_str())) {
                        out.insert(k.clone(), Value::String(s.clone()));
                    }
                } else if prop_schema["x-opaque-handle"].as_bool() == Some(true) {
                    out.insert(k.clone(), Value::String(handles.store(s)));
                }
            }
            _ => {}
        }
    }
    Value::Object(out)
}

/// The extraction schema `run` reads untrusted output under, when it is
/// quarantined: its own, or the run's at the top of its parents (B2).
pub fn quarantine_of(s: &State, run: &str) -> Option<Value> {
    let r = s.runs.get(run)?;
    r.config.quarantine.clone().or_else(|| s.requester_run(r).config.quarantine)
}

/// Every string inside `v`, with the field it is under.
fn strings<'a>(v: &'a Value, key: &'a str, out: &mut Vec<(&'a str, &'a str)>) {
    match v {
        Value::String(s) => out.push((key, s)),
        Value::Array(a) => a.iter().for_each(|x| strings(x, key, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| strings(x, k, out)),
        _ => {}
    }
}

/// The destination field a handle stands in, if one does: a field that says
/// where the action goes, or an address written anywhere (`$q1@x.example`).
fn handle_in_destination(input: &Value) -> Option<String> {
    let mut all = vec![];
    strings(input, "", &mut all);
    all.into_iter().find_map(|(key, s)| {
        let named = DESTINATION_FIELDS.contains(&key.to_ascii_lowercase().as_str()) && contains_opaque_handle(s);
        (named || crate::tools::addresses_in(s).iter().any(|a| contains_opaque_handle(a))).then(|| key.to_string())
    })
}

fn expand_value(v: &mut Value, handles: &BTreeMap<String, String>) {
    match v {
        Value::String(s) => *s = expand_body_handles(handles, s),
        Value::Array(a) => a.iter_mut().for_each(|x| expand_value(x, handles)),
        Value::Object(o) => o.values_mut().for_each(|x| expand_value(x, handles)),
        _ => {}
    }
}

impl Kernel {
    /// Returns `true` if `run_id` or any ancestor in its `requester_run` lineage
    /// is executing under `quarantine_untrusted_reads` (B2).
    pub(crate) fn run_is_quarantined(&self, run_id: &str) -> bool {
        self.read(|s| quarantine_of(s, run_id).is_some())
    }

    /// The schema a firing of `sc` that writes `text` reads untrusted output
    /// under: unattended, and with a writable destination someone other
    /// than its person, it is quarantined; its own schema, or the typed-only
    /// default. Decided before the run is created, and recorded with it.
    pub(crate) fn schedule_quarantine(&self, sc: &Schedule, text: &str) -> Option<Value> {
        let (_, dests) = crate::envelope::resolve::extract_literal_destinations(text);
        let env = Envelope { scope_id: sc.id.clone(), allowed_destinations: dests, allowed_write_apps: Default::default(), user_named: vec![], grant_ids: vec![] };
        let owner_email = self.person(&sc.person).map(|p| p.id).filter(|id| id.contains('@'));
        crate::quarantine::routine_needs_quarantine(&env, owner_email.as_deref())
            .then(|| self.read(|s| s.arrangements.quarantine_schema(&sc.id).cloned()).unwrap_or_else(default_typed_schema))
    }

    /// Quarantine an untrusted tool result for a quarantined run: extracts typed
    /// fields and keeps the raw text behind an opaque handle (`$qN`), so the
    /// planner never reads raw untrusted prose (B1). Returns what the model
    /// is told, and the result's data with the new handles in it.
    pub(crate) async fn quarantine_tool_output(&self, run: &Run, tool: &str, raw_text: &str, data: Option<Value>) -> (String, Option<Value>) {
        let Some(schema) = self.read(|s| quarantine_of(s, &run.id)) else { return (raw_text.to_string(), data) };
        let root = self.requester_run(run);
        let extracted = self.quarantined_extract(run, raw_text, &schema, &format!("Extract declared typed fields from {tool} output.")).await.unwrap_or_else(|_| json!({}));
        // Numbered after the tree's handles as they stand now, just before
        // this result is recorded: a run's calls go one at a time.
        let mut handles = Handles::after(self.read(|s| handles_of(s, &root.id).len()));
        let typed = sanitize_with_handles(&mut handles, &extracted, &schema);
        let body_handle = handles.store(raw_text);
        let told = json!({
            "quarantined": true,
            "tool": tool,
            "extracted": typed,
            "opaque_body_handle": body_handle,
            "note": "Raw untrusted text is stored behind opaque_body_handle; you may place that handle in a message body template, never in a destination field."
        });
        (told.to_string(), handles.into_data(data))
    }

    /// The action as it reaches the app, for a quarantined run whose input
    /// names opaque handles: refused when a handle stands where it says
    /// where the action goes (N12); otherwise, in an action that changes
    /// something and reaches others (`leaves`: not the person's own space,
    /// nor a sub-agent's goal), each handle becomes its text, here at the
    /// boundary and nowhere before. `None` leaves the action as it is.
    pub(crate) fn expand_handles(&self, a: &Action, leaves: bool) -> Result<Option<Action>, String> {
        let mut all = vec![];
        strings(&a.input, "", &mut all);
        if !all.iter().any(|(_, s)| contains_opaque_handle(s)) || !self.run_is_quarantined(&a.run) {
            return Ok(None);
        }
        if let Some(field) = handle_in_destination(&a.input) {
            return Err(format!("an opaque handle stands in where this goes ({field}); a handle may stand only in a message body, never in a destination"));
        }
        if !leaves {
            return Ok(None);
        }
        let handles = self.read(|s| s.runs.get(&a.run).map(|r| handles_of(s, &s.requester_run(r).id))).unwrap_or_default();
        let mut input = a.input.clone();
        expand_value(&mut input, &handles);
        Ok(Some(Action { run: a.run.clone(), call_id: a.call_id.clone(), step_id: a.step_id.clone(), tool: a.tool.clone(), input, issuer: a.issuer }))
    }
}

#[cfg(test)]
#[path = "quarantine_runtime_tests.rs"]
mod tests;
