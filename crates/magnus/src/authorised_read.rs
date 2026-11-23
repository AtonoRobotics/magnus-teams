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
use crate::tools::{Audience, Label, Risk};
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

impl Refused {
    /// Why, in a word, with nothing the app said: for a log line.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Refused::Unknown => "unknown",
            Refused::NotARead => "not a read",
            Refused::Held(_) => "held",
            Refused::NoRights => "no rights",
            Refused::Pin(_) => "definition not screened",
            Refused::Denied(_) => "denied by policy",
            Refused::Asks(_) => "policy asks",
            Refused::Unjudged(_) => "not judged",
            Refused::NotConnected => "not connected",
            Refused::Secret(_) => "sign-in unreadable",
            Refused::Failed(_) => "the app refused",
            Refused::Call(_) => "the call failed",
            Refused::Audit(_) => "not audited",
        }
    }
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
        let rf = facts(self, &format!("{}__{}", r.app, r.read));
        self.read_as(r, rf).await
    }

    /// The login a connected password manager holds for a site, read for
    /// `person`'s run. A password manager's tools are the kernel's alone,
    /// never offered to the agent (`boundary::facts` classes them
    /// prohibited), so this one read is classed a read here; otherwise it
    /// is authorised and audited as every other read the server makes for a
    /// person: the switches, their groups, the screened definition, policy.
    /// Nothing it answers is logged.
    pub(crate) async fn password_manager_read(&self, run: &str, person: &str, app: &str, tool: &str, args: Value) -> Result<Value, Refused> {
        let is_manager = self.cfg.computer.as_ref().is_some_and(|c| c.password_manager.as_deref() == Some(app));
        let (Some(p), Some(r)) = (self.person(person), self.read(|s| s.runs.get(run).cloned())) else { return Err(Refused::Unknown) };
        let name = format!("{app}__{tool}");
        let rf = self.connector(app).filter(|_| is_manager).and_then(|c| {
            let label = c.tools.get(tool).and_then(|cl| cl.label.as_deref()).and_then(Label::parse).or_else(|| Label::parse(&c.label))?;
            Some(Facts {
                name: name.clone(),
                app: app.into(),
                risk: Risk::Read,
                label,
                audience: Audience::parse(&c.audience)?,
                connector: Some(c),
                remote: tool.into(),
                reaches_self: false,
            })
        });
        let action = Action {
            run: run.into(),
            call_id: crate::kernel::new_id("login"),
            step_id: format!("{}.password_manager", r.replies.len()),
            tool: name,
            input: args.clone(),
            issuer: Issuer::Model,
        };
        let within = Within { run: &r, action: &action, prefix: "password_manager", stage: "password_manager", what: "password manager read" };
        self.read_as(SideRead { person: &p, app, read: tool, args, within: Some(within) }, rf).await
    }

    async fn read_as(&self, r: SideRead<'_>, rf: Option<Facts>) -> Result<Value, Refused> {
        let name = format!("{}__{}", r.app, r.read);
        let run = r.within.as_ref().map(|w| w.run);
        self.read(|s| precheck(s, r.person, rf.as_ref(), run))?;
        let Some(rf) = rf else { return Err(Refused::Unknown) };
        let Some(c) = rf.connector.clone() else { return Err(Refused::Unknown) };
        let run_id = run.map_or("", |r| r.id.as_str());
        // Whose sign-in it reads through: in a team thread, the workspace's (team/sign_in.rs).
        let owner = run.map_or_else(|| r.person.id.clone(), |w| self.app_owner(w, &c));
        self.check_pin(&c, &rf, &owner, run_id).await.map_err(|(why, _)| Refused::Pin(why))?;
        let judged = judge(&self.read(|s| s.org.policies().clone()), r.person, &rf, run)?;
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
            Some(_) => match self.connector_secret(&c, &owner).await {
                Ok(Some(s)) => Some(s),
                Ok(None) => return Err(Refused::NotConnected),
                Err(e) => return Err(Refused::Secret(format!("{e:#}"))),
            },
            None => None,
        };
        let out = self.connector_call(&c, &owner, r.app, secret.as_ref().map(|s| s.expose()), r.read, &r.args).await.map_err(Refused::Call)?;
        if out.is_error {
            return Err(Refused::Failed(out.text));
        }
        Ok(out.structured.or_else(|| serde_json::from_str(&out.text).ok()).unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod password_manager_tests;
#[cfg(test)]
mod tests;
