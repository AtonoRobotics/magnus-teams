//! The reads the gate makes for a send, through `Kernel::authorised_read`:
//! where a draft goes (#184) and whether the person has mailed an address
//! before (#210). Each keeps only what it does with the answer and how it
//! says a refusal.

use crate::apps::CallError;
use crate::authorised_read::{Refused, SideRead, Within};
use crate::boundary::{Action, Facts, Shown};
use crate::config::Person;
use crate::kernel::Kernel;
use crate::state::Run;
use crate::tools;
use anyhow::Result;
use serde_json::{json, Value};

/// Whether a sent search found mail: what `found` points at is not empty.
pub(crate) fn found_at(read: &Value, found: &str) -> bool {
    match read.pointer(found).unwrap_or(&Value::Null) {
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::String(s) => !s.trim().is_empty(),
        _ => true,
    }
}

/// How the gate asks whether the person has mailed an address before, a
/// recipient-provenance source beside the person's own words (#210):
/// `read` (a read tool of the same app) is called with `input` set to
/// `query` with `{address}` replaced, through the person's own sign-in,
/// and the address counts as mailed when `found` (a JSON pointer into
/// what it returns) is not empty.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct SentSearch {
    pub read: String,
    pub input: String,
    pub query: String,
    pub found: String,
}

impl SentSearch {
    /// What is wrong with this declaration, given each tool's risk class:
    /// it reads with a read tool of the same app, names its input, asks
    /// about the address, and points at where the answer says found.
    pub fn fault(&self, risk: impl Fn(&str) -> Option<String>) -> Option<String> {
        if risk(&self.read).as_deref() != Some("read") {
            return Some(format!("sent mail is searched with {}, which is not a read tool of the same app", self.read));
        }
        if self.input.is_empty() || self.query.is_empty() || self.found.is_empty() {
            return Some("sent mail names no input, query or found pointer".into());
        }
        if !self.query.contains("{address}") {
            return Some("sent mail's query does not ask about {address}".into());
        }
        if !self.found.starts_with('/') {
            return Some(format!("sent mail's {:?} is not a JSON pointer", self.found));
        }
        None
    }
}

/// Where an action goes, read at the gate from the object it acts on and
/// never taken from what the model wrote (approvals spec, Destination
/// fields): `read` is the app's read tool for that object, called with the
/// action's `id` field as it is; `recipients` and `content` point (JSON
/// pointers) into what it returns. The approval card shows them, and an
/// approval holds only for exactly what they were.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct Destination {
    pub read: String,
    pub id: String,
    pub recipients: Vec<String>,
    #[serde(default)]
    pub content: Vec<String>,
    /// Fields the approval holds for but the card does not show: a digest
    /// of every part of the message, so a change to a part the card does
    /// not show (an HTML part, an attachment's bytes) asks again (#177).
    #[serde(default)]
    pub bound: Vec<String>,
}

impl Destination {
    /// What is wrong with this declaration for `tool`, given each tool's
    /// risk class: it is for a write, it reads with a read tool of the same
    /// app, and each pointer is one and names its own field.
    pub fn fault(&self, tool: &str, risk: impl Fn(&str) -> Option<String>) -> Option<String> {
        let r = risk(tool)?;
        if r == "read" {
            return Some(format!("{tool} is a read: it goes nowhere"));
        }
        if risk(&self.read).as_deref() != Some("read") {
            return Some(format!("{tool} reads its destinations with {}, which is not a read tool of the same app", self.read));
        }
        if self.id.is_empty() || self.recipients.is_empty() {
            return Some(format!("{tool} names no id field or no recipients"));
        }
        let mut names = std::collections::BTreeSet::new();
        for p in self.recipients.iter().chain(&self.content).chain(&self.bound) {
            let name = p.rsplit('/').next().unwrap_or_default();
            if !p.starts_with('/') || name.is_empty() || !names.insert(name) {
                return Some(format!("{tool}: {p:?} is not a JSON pointer ending in a field name of its own"));
            }
        }
        None
    }
}

impl Kernel {
    /// Read where an action goes from what it acts on, when its tool says
    /// how (a draft's To, Cc and Bcc through `get_draft`, never the model's
    /// words), with the person's own sign-in. Whatever stops the read stops
    /// the action: nothing is sent to an address no one has seen.
    pub(crate) async fn destinations(&self, a: &Action, f: &Facts, run: &Run, person: &Person) -> std::result::Result<Option<Shown>, String> {
        let Some(d) = f.connector.as_ref().and_then(|c| c.destinations.get(&f.remote)) else { return Ok(None) };
        let nothing = format!("Nothing was sent to {}.", f.app);
        let Some(id) = a.input.get(&d.id).filter(|v| !v.is_null()) else {
            return Err(format!("{} names no {}, so who it goes to cannot be read. {nothing}", a.tool, d.id));
        };
        // The preview read is authorized like any read (#184): its own
        // holds, access check, pin validation and policy decision, audited
        // as a Decision of its own. Nothing read here reaches the model,
        // but the read still goes to the provider, so a task layer
        // forbidding it denies the send. Ask fails closed the same way: the
        // send cannot show where it goes without the read.
        let read_name = format!("{}__{}", f.app, d.read);
        let within = Within { run, action: a, prefix: "preview", stage: "destinations", what: "destination read" };
        let read = SideRead { person, app: &f.app, read: &d.read, args: json!({ d.id.as_str(): id }), within: Some(within) };
        let read = self.authorised_read(read).await.map_err(|r| {
            let (app, tool) = (&f.app, &a.tool);
            match r {
                Refused::Unknown => format!("{app}'s destination read {} is not a known classified tool, so who {tool} goes to cannot be read. {nothing}", d.read),
                Refused::NotARead => format!("{app}'s destination read {} is not classed as a read, so who {tool} goes to cannot be read. {nothing}", d.read),
                Refused::Held(h) => format!("{}. {nothing}", h.said()),
                Refused::NoRights => format!("{} does not have access to {app} (identity-provider groups), so who {tool} goes to cannot be read. {nothing}", person.id),
                Refused::Pin(why) => format!("{why}, so who {tool} goes to cannot be read. {nothing}"),
                Refused::Denied(reason) => format!("policy forbids the destination read ({read_name}): {reason}. {nothing}"),
                Refused::Asks(_) => format!("policy asks before the destination read ({read_name}), so who {tool} goes to cannot be shown without it. {nothing}"),
                Refused::Unjudged(e) => format!("the destination read ({read_name}) cannot be judged: {e}. {nothing}"),
                Refused::NotConnected => {
                    format!("{app} is not connected for the person this task is for (or their sign-in expired), so who {tool} goes to cannot be read. {nothing}")
                }
                Refused::Secret(e) => format!("{e}. {nothing}"),
                Refused::Failed(text) => format!("{app} could not read {id} to see who {tool} goes to: {text}. {nothing}"),
                Refused::Call(CallError::NotSent(m) | CallError::Unknown(m) | CallError::BadAnswer(m)) => {
                    format!("{app} could not be read to see who {tool} goes to: {m}. {nothing}")
                }
                Refused::Call(CallError::NeedsScope(_)) => format!("{app} wants more access to read who {tool} goes to. {nothing}"),
                Refused::Audit(e) => format!("{e:#}"),
            }
        })?;
        let field = |p: &String| (p.rsplit('/').next().unwrap_or_default().to_string(), read.pointer(p).cloned().unwrap_or(Value::Null));
        let recipients: std::collections::BTreeMap<String, Vec<String>> = d.recipients.iter().map(field).map(|(k, v)| (k, tools::addresses(&v))).collect();
        if recipients.values().all(|v| v.is_empty()) {
            return Err(format!("{id} names no recipients at {}, so {} would reach no one Magnus can show. {nothing}", f.app, a.tool));
        }
        Ok(Some(Shown { recipients, content: d.content.iter().map(field).collect(), bound: d.bound.iter().map(field).collect() }))
    }

    /// Whether the person has mailed `address` before (#210): the
    /// manifest's sent search, through their own sign-in. The sent search
    /// is authorized like any read (#371), as for destination previews
    /// (#184). Whatever stops the read counts as not mailed: the send asks
    /// rather than passing on an unchecked claim. Only a decision the log
    /// cannot keep is an error.
    pub(crate) async fn mailed_before(&self, a: &Action, f: &Facts, run: &Run, person: &Person, address: &str) -> Result<bool> {
        let Some(s) = f.connector.as_ref().and_then(|c| c.sent.as_ref()) else { return Ok(false) };
        let within = Within { run, action: a, prefix: "sent", stage: "mailed_before", what: "sent search" };
        let read = SideRead { person, app: &f.app, read: &s.read, args: json!({ s.input.as_str(): s.query.replace("{address}", address) }), within: Some(within) };
        match self.authorised_read(read).await {
            Ok(read) => Ok(found_at(&read, &s.found)),
            Err(Refused::Audit(e)) => Err(e),
            // A hold or a policy's own answer is already plain; the rest is
            // worth a line in the server's log.
            Err(Refused::Held(_) | Refused::Denied(_) | Refused::Asks(_)) => Ok(false),
            Err(r) => {
                tracing::warn!("sent mail for {}: the search did not answer: {r:?}", person.id);
                Ok(false)
            }
        }
    }
}
