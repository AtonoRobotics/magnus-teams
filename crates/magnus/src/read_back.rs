//! Read-back (spec rev 147): after each irreversible action the server reads
//! the world back, for example the sent message in the sent folder, and
//! attaches what it found. A claim that does not match is caught there.
//!
//! A connector says how, per tool, in `[read_back.<tool>]`: the read tool,
//! its input (each field a JSON pointer into what the action had, or a
//! literal), and what must match (a pointer into what the action had, and
//! one into what the read returned). What the action had is
//! `{ "input": …, "result": …, "shown": … }`: its input, what it returned,
//! and the recipients and content the gate read before it ran.
//!
//! The read goes through the boundary as the server's own (`Issuer::ReadBack`),
//! which may only read. What it found is the person's to see, beside the
//! result. The agent is told only whether it matched and which field did
//! not, never the content: what was read is not given to it as words.
use crate::boundary::{Action, Facts, Issuer, Shown, Verdict};
use crate::events::Ev;
use crate::evidence::Change;
use crate::kernel::Kernel;
use crate::tools::Risk;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How the world is read back after one tool's action.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReadBack {
    /// A read tool of the same app.
    pub read: String,
    /// The read's input: each field a pointer (`/result/message_id`) into
    /// what the action had, or a literal that does not start with `/`.
    #[serde(default)]
    pub input: BTreeMap<String, String>,
    /// What must match: a pointer into what the action had, to one into
    /// what the read returned.
    #[serde(default)]
    pub expect: BTreeMap<String, String>,
}

impl ReadBack {
    /// What is wrong with this declaration for `tool`, given each tool's
    /// risk class: it follows a write, it reads with a read tool of the same
    /// app, and each pointer is one.
    pub fn fault(&self, tool: &str, risk: impl Fn(&str) -> Option<String>) -> Option<String> {
        let r = risk(tool)?;
        if r == "read" {
            return Some(format!("{tool} is a read: there is nothing to read back"));
        }
        if risk(&self.read).as_deref() != Some("read") {
            return Some(format!("{tool} reads back with {}, which is not a read tool of the same app", self.read));
        }
        for (from, to) in &self.expect {
            if !from.starts_with('/') || !to.starts_with('/') {
                return Some(format!("{tool}: read_back expects {from:?} = {to:?}; both are JSON pointers"));
            }
        }
        None
    }
}

/// Whether what the action had (`want`) is in what was read back (`got`):
/// a string is contained (a header holds a name and the address), a list
/// matches when each of its items does, anything else is equal.
pub fn holds(want: &Value, got: &Value) -> bool {
    match (want, got) {
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::Array(ws), _) => ws.iter().all(|w| holds(w, got)),
        (w, Value::Array(gs)) => gs.iter().any(|g| holds(w, g)),
        (Value::String(w), Value::String(g)) => g.to_lowercase().contains(&w.trim().to_lowercase()),
        (w, g) => w == g,
    }
}

/// The read's input, from what the action had.
fn read_input(rb: &ReadBack, had: &Value) -> Value {
    let m: serde_json::Map<String, Value> =
        rb.input.iter().map(|(k, v)| (k.clone(), if v.starts_with('/') { had.pointer(v).cloned().unwrap_or(Value::Null) } else { Value::String(v.clone()) })).collect();
    Value::Object(m)
}

/// Which expected fields what was read does not hold, by name.
fn differs(rb: &ReadBack, had: &Value, found: &Value) -> Vec<String> {
    rb.expect
        .iter()
        .filter(|(from, to)| !had.pointer(from).zip(found.pointer(to)).is_some_and(|(want, got)| matches_field(from, want, got)))
        .map(|(from, _)| from.rsplit('/').next().unwrap_or(from).to_string())
        .collect()
}

/// Recipient headers are address sets; content must survive unchanged.
fn matches_field(from: &str, want: &Value, got: &Value) -> bool {
    if want.is_null() || got.is_null() {
        return false;
    }
    if from.starts_with("/shown/recipients/") {
        let expected = crate::tools::addresses(want);
        let actual = crate::tools::addresses(got);
        return expected.into_iter().collect::<std::collections::BTreeSet<_>>() == actual.into_iter().collect();
    }
    if from.starts_with("/shown/content/") || from.starts_with("/bound/") {
        return !want.is_null() && want == got;
    }
    holds(want, got)
}

/// Whether an action is read back: the model's own, irreversible or a
/// payment, and it went through.
pub(crate) fn applies(a: &Action, f: &Facts, is_error: bool) -> bool {
    a.issuer == Issuer::Model && !is_error && f.connector.is_some() && matches!(f.risk, Risk::Irreversible | Risk::Financial)
}

impl Kernel {
    /// Read the world back after an irreversible action, record what was
    /// found, and say in one line, for the action's result, how it went.
    pub(crate) async fn read_back(self: &Arc<Self>, a: &Action, f: &Facts, shown: Option<&Shown>, data: Option<&Value>) -> Result<String> {
        let rb = f.connector.as_ref().and_then(|c| c.read_back.get(&f.remote)).cloned();
        let record = |read: Option<String>, found: Value, matches: Option<bool>, note: String| {
            Ev::Evidence(Change::ReadBack { run: a.run.clone(), call_id: a.call_id.clone(), tool: a.tool.clone(), read, found, matches, note })
        };
        let Some(rb) = rb else {
            let note = format!("Not read back: {} has no way set up to read {} back.", f.app, a.tool);
            self.emit("kernel", record(None, Value::Null, None, note.clone())).await?;
            return Ok(note);
        };
        let had = json!({ "input": a.input, "result": data.cloned().unwrap_or(Value::Null), "shown": shown.map(Shown::details), "bound": shown.map(|s| &s.bound) });
        let read = format!("{}__{}", f.app, rb.read);
        let check = Action {
            run: a.run.clone(),
            call_id: format!("readback:{}", a.call_id),
            step_id: format!("{}.readback", a.step_id),
            tool: read.clone(),
            input: read_input(&rb, &had),
            issuer: Issuer::ReadBack,
        };
        let v = Box::pin(self.execute(check)).await?;
        let (note, found, matches) = match v {
            Verdict::Done { is_error: false, text, data } => {
                let found = data.unwrap_or_else(|| serde_json::from_str(&text).unwrap_or(Value::String(text)));
                let off = differs(&rb, &had, &found);
                if off.is_empty() {
                    (format!("Read back with {}: it matches what was done.", rb.read), found, Some(true))
                } else {
                    (format!("Read back with {}: it does not match what was done ({} differs). Do not report this as done.", rb.read, off.join(", ")), found, Some(false))
                }
            }
            Verdict::Done { text, .. } => (format!("Not read back: {} could not be read ({}).", rb.read, first_line(&text)), Value::Null, None),
            Verdict::Suspended => (format!("Not read back: {} was held.", rb.read), Value::Null, None),
        };
        self.emit("kernel", record(Some(rb.read.clone()), found, matches, note.clone())).await?;
        Ok(note)
    }
}

fn first_line(t: &str) -> &str {
    t.lines().next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_done_is_found_in_what_was_read() {
        assert!(holds(&json!("Q3 numbers"), &json!("Re: Q3 numbers")));
        assert!(holds(&json!(["bob@corp.example"]), &json!("Bob <bob@corp.example>")));
        assert!(holds(&json!(["bob@corp.example"]), &json!(["Bob <BOB@corp.example>", "x@y.example"])));
        assert!(!holds(&json!(["bob@corp.example", "eve@evil.example"]), &json!("Bob <bob@corp.example>")));
        assert!(holds(&json!("closed"), &json!("closed")) && !holds(&json!("closed"), &json!("open")));
        assert!(!holds(&json!(true), &Value::Null), "a field the read lacks matched");
    }

    #[test]
    fn a_read_back_names_the_field_that_differs() {
        let rb = ReadBack {
            read: "get_record".into(),
            input: BTreeMap::from([("id".into(), "/input/id".into()), ("view".into(), "full".into())]),
            expect: BTreeMap::from([("/input/fields/status".into(), "/status".into())]),
        };
        let had = json!({ "input": { "id": "42", "fields": { "status": "closed" } } });
        assert_eq!(read_input(&rb, &had), json!({ "id": "42", "view": "full" }));
        assert!(differs(&rb, &had, &json!({ "status": "closed" })).is_empty());
        assert_eq!(differs(&rb, &had, &json!({ "status": "open" })), vec!["status".to_string()]);
    }

    #[test]
    fn gmail_checks_all_recipients_and_visible_content() {
        let manifest: toml::Value = toml::from_str(include_str!("../../../catalog/google-gmail.toml")).unwrap();
        let rb: ReadBack = manifest["read_back"]["send_draft"].clone().try_into().unwrap();
        let had = json!({"bound": {"digest": "parts-hash"}, "shown": {"recipients": {"to": ["a@example.com"], "cc": ["c@example.com"], "bcc": ["b@example.com"]}, "content": {"subject": "Subject", "body": "Exact body", "attachments": [{"filename": "a.txt", "size": 12}]}}});
        let got = json!({"digest": "parts-hash", "to": "A <a@example.com>", "cc": "c@example.com", "bcc": "b@example.com", "subject": "Subject", "body": "Exact body", "attachments": [{"filename": "a.txt", "size": 12}]});
        assert!(differs(&rb, &had, &got).is_empty());
        for key in ["to", "cc", "bcc", "subject", "body", "attachments", "digest"] {
            let mut changed = got.clone();
            changed[key] = json!("wrong");
            assert!(!differs(&rb, &had, &changed).is_empty(), "{key} was not checked");
        }
        let mut extra = got.clone();
        extra["to"] = json!("a@example.com, extra@example.com");
        assert!(!differs(&rb, &had, &extra).is_empty());
        extra = got.clone();
        extra["body"] = json!("Exact body with injected content");
        assert!(!differs(&rb, &had, &extra).is_empty());
    }

    #[test]
    fn missing_expected_fields_never_match() {
        let rb = ReadBack { read: "get".into(), input: BTreeMap::new(), expect: BTreeMap::from([("/missing".into(), "/status".into())]) };
        assert!(!differs(&rb, &json!({}), &json!({"status": "closed"})).is_empty());
        assert!(!differs(&rb, &json!({}), &json!({})).is_empty());
        assert!(!holds(&Value::Null, &Value::Null));
    }

    #[test]
    fn a_read_back_must_read_with_a_read_tool() {
        let rb = ReadBack { read: "update_record".into(), input: BTreeMap::new(), expect: BTreeMap::new() };
        let risk = |t: &str| Some(if t.starts_with("get") { "read" } else { "irreversible" }.to_string());
        assert!(rb.fault("send", risk).is_some_and(|w| w.contains("not a read tool")));
        let ok = ReadBack { read: "get_record".into(), ..rb };
        assert_eq!(ok.fault("send", risk), None);
        assert!(ok.fault("get_record", risk).is_some(), "a read was given a read-back");
    }
}
