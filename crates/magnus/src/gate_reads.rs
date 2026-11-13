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
