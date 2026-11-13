//! One authorised read of a person's app that the model did not ask for:
//! a send's destination preview (#184), the sent search behind "mailed
//! before" (#210, #371) and a watched read's poll (#406). Each is checked
//! as the gate checks any call (decisions/2026-11-13-one-authorised-read.md):
//! the read is one the app classes as a read, no hold stops the person's
//! work on that app (the server's switch, theirs, their agent's, the app's,
//! their account turned off), they may use the app, its live definition is
//! the screened one, and policy allows it. Inside a run the decision is
//! audited as a Decision of its own. Only then does it go out, through the
//! person's own sign-in. Callers keep only what they do with the answer.

use crate::apps::CallError;
use crate::boundary::{facts, has_rights, Action, Facts, Issuer};
use crate::config::Person;
use crate::halts::{self, Hold, Phase, Subject};
use crate::kernel::Kernel;
use crate::policy::{self, Outcome, ToolFacts};
use crate::state::{Run, State};
use crate::tools::Risk;
use serde_json::Value;
use std::collections::BTreeMap;

/// The run a read is done inside, and how its decision is audited.
pub(crate) struct Within<'a> {
    pub run: &'a Run,
    /// The model's action the read serves.
    pub action: &'a Action,
    /// The audited call's id is `<prefix>:<the action's call id>`.
    pub prefix: &'a str,
    /// The audit's stage: `destinations`, `mailed_before`.
    pub stage: &'a str,
    /// The read, as an ask's reason names it: "destination read".
    pub what: &'a str,
}

/// One read the server makes for a person.
pub(crate) struct SideRead<'a> {
    pub person: &'a Person,
    pub app: &'a str,
    /// The app's own name for the read tool.
    pub read: &'a str,
    pub args: Value,
    /// None for a poll, which is the kernel's own read and no run's.
    pub within: Option<Within<'a>>,
}

/// Why a read did not go out, or did not answer.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The app classes no such tool.
    Unknown,
    /// The app classes it, but not as a read.
    NotARead,
    Held(Hold),
    NoRights,
    /// Its live definition is not the screened one.
    Pin(String),
    Denied(String),
    /// Policy asks before it: no one is there to ask.
    Asks(String),
    Unjudged(String),
    NotConnected,
    Secret(String),
    /// The app answered with an error.
    Failed(String),
    Call(CallError),
    /// The decision could not be written to the log.
    Audit(anyhow::Error),
}

/// What policy said of a read, once the checks before it passed.
#[derive(Debug, PartialEq)]
pub(crate) enum Judged {
    Allow(String),
    Deny(String, Option<String>),
    Ask(String),
}

/// The checks that need no answer from anyone, in order: the read is
/// classed a read, no hold stops it, the person may use the app.
pub(crate) fn precheck(s: &State, person: &Person, rf: Option<&Facts>, within: Option<&Run>) -> Result<(), Refused> {
    let rf = rf.ok_or(Refused::Unknown)?;
    if rf.risk != Risk::Read {
        return Err(Refused::NotARead);
    }
    let who = match within {
        Some(run) => Subject::of_run(s, run, Some(&rf.name)),
        None => Subject::person_app(&person.id, Some(&rf.app)),
    };
    if let Some(h) = halts::may_proceed(s, &who, Phase::Call) {
        return Err(Refused::Held(h));
    }
    if !has_rights(person, rf) {
        return Err(Refused::NoRights);
    }
    Ok(())
}

/// Policy's judgement of the read: inside a run, under its task's layers
/// and its taint; a poll under the person's own layers, untainted.
pub(crate) fn judge(policies: &BTreeMap<String, String>, person: &Person, rf: &Facts, within: Option<&Run>) -> Result<Judged, Refused> {
    let (task, untrusted, private) = within.map_or((None, false, false), |r| (Some(r.task.as_str()), r.untrusted, r.private));
    match policy::decide(policies, person, task, &ToolFacts::from(rf), untrusted, private) {
        Ok(Outcome::Allow { rule }) => Ok(Judged::Allow(rule)),
        Ok(Outcome::Deny { reason, rule }) => Ok(Judged::Deny(reason, rule)),
        Ok(Outcome::Ask { rule }) => Ok(Judged::Ask(rule)),
        Err(e) => Err(Refused::Unjudged(format!("{e:#}"))),
    }
}

impl Kernel {
    /// Make one read for a person, authorised as the gate authorises any
    /// call, and return its answer.
    pub(crate) async fn authorised_read(&self, r: SideRead<'_>) -> Result<Value, Refused> {
        let name = format!("{}__{}", r.app, r.read);
        let rf = facts(self, &name);
        let run = r.within.as_ref().map(|w| w.run);
        self.read(|s| precheck(s, r.person, rf.as_ref(), run))?;
        let Some(rf) = rf else { return Err(Refused::Unknown) };
        let Some(c) = rf.connector.clone() else { return Err(Refused::Unknown) };
        let run_id = run.map_or("", |r| r.id.as_str());
        self.check_pin(&c, &rf, &r.person.id, run_id).await.map_err(|(why, _)| Refused::Pin(why))?;
        let judged = judge(&self.read(|s| s.policies.clone()), r.person, &rf, run)?;
        if let Some(w) = &r.within {
            let audited = Action {
                run: w.action.run.clone(),
                call_id: format!("{}:{}", w.prefix, w.action.call_id),
                step_id: w.action.step_id.clone(),
                tool: name.clone(),
                input: r.args.clone(),
                issuer: Issuer::Model,
            };
            let (outcome, reason, rule) = match &judged {
                Judged::Allow(rule) => ("allow", "permitted".to_string(), Some(rule.clone())),
                Judged::Deny(reason, rule) => ("deny", reason.clone(), rule.clone()),
                Judged::Ask(rule) => ("deny", format!("policy asks before the {} ({rule})", w.what), Some(rule.clone())),
            };
            self.decision(&audited, outcome, w.stage, &reason, rule).await.map_err(Refused::Audit)?;
        }
        match judged {
            Judged::Allow(_) => {}
            Judged::Deny(reason, _) => return Err(Refused::Denied(reason)),
            Judged::Ask(rule) => return Err(Refused::Asks(rule)),
        }
        let secret = match &c.credential {
            Some(_) => match self.connector_secret(&c, &r.person.id).await {
                Ok(Some(s)) => Some(s),
                Ok(None) => return Err(Refused::NotConnected),
                Err(e) => return Err(Refused::Secret(format!("{e:#}"))),
            },
            None => None,
        };
        let out = self.connector_call(&c, &r.person.id, r.app, secret.as_ref().map(|s| s.expose()), r.read, &r.args).await.map_err(Refused::Call)?;
        if out.is_error {
            return Err(Refused::Failed(out.text));
        }
        Ok(out.structured.or_else(|| serde_json::from_str(&out.text).ok()).unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests;
