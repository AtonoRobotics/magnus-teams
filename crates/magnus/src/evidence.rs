//! Evidence comes from the record, not from ceremony (spec rev 147).
//!
//! No contract, criteria list or plan is required before the agent acts or
//! reports. The server already records every tool call and boundary action,
//! so the evidence a result carries is a byproduct of the work:
//! - receipts: what was done, in terms the boundary log proves (`receipts`);
//! - read-backs: after each irreversible action the server reads the world
//!   back and keeps what it found (`read_back.rs`, `Change::ReadBack`);
//! - sources: each claim in a result links to the call it came from
//!   (`Change::Sources`, checked by `cited`).
//!
//! A task's card shows a live status line while it works (`status_line`),
//! and becomes the result when it ends.
use crate::state::{Run, State};
use crate::tools::Risk;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The most sources one result may cite.
pub const MAX_SOURCES: usize = 40;
/// The longest claim a source is cited for, in characters.
pub const MAX_CLAIM_CHARS: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// What the server read back after an irreversible action. `read` is
    /// the read tool it used; None when none is set up for the action's
    /// tool, which `note` then says. `matches` is None when nothing could
    /// be compared.
    ReadBack {
        run: String,
        call_id: String,
        tool: String,
        #[serde(default)]
        read: Option<String>,
        #[serde(default)]
        found: Value,
        #[serde(default)]
        matches: Option<bool>,
        #[serde(default)]
        note: String,
    },
    /// The sources a finished result cites: each claim and the call it
    /// came from.
    Sources { run: String, sources: Vec<Source> },
}

impl Change {
    pub fn run(&self) -> &str {
        match self {
            Change::ReadBack { run, .. } | Change::Sources { run, .. } => run,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    pub claim: String,
    pub call: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReadBack {
    pub call_id: String,
    pub tool: String,
    pub read: Option<String>,
    pub found: Value,
    pub matches: Option<bool>,
    pub note: String,
    pub at: DateTime<Utc>,
}

/// Each run's read-backs and cited sources.
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    read_backs: HashMap<String, Vec<ReadBack>>,
    sources: HashMap<String, Vec<Source>>,
}

impl Ledger {
    pub fn apply(&mut self, at: DateTime<Utc>, c: &Change) {
        match c {
            Change::ReadBack { run, call_id, tool, read, found, matches, note } => self.read_backs.entry(run.clone()).or_default().push(ReadBack {
                call_id: call_id.clone(),
                tool: tool.clone(),
                read: read.clone(),
                found: found.clone(),
                matches: *matches,
                note: note.clone(),
                at,
            }),
            Change::Sources { run, sources } => {
                self.sources.insert(run.clone(), sources.clone());
            }
        }
    }

    pub fn read_backs(&self, run: &str) -> &[ReadBack] {
        self.read_backs.get(run).map_or(&[], Vec::as_slice)
    }

    pub fn sources(&self, run: &str) -> &[Source] {
        self.sources.get(run).map_or(&[], Vec::as_slice)
    }
}

/// A tool's app and risk class, as the boundary knows it.
pub type Class<'a> = &'a dyn Fn(&str) -> Option<(String, Risk)>;

/// What a run did, in terms the boundary log proves: each action the model
/// asked for that changes something and went through, with where it went
/// and what the server read back after it. Nothing here rests on the
/// agent's word. What was refused or failed is the result's "not done".
pub fn receipts(s: &State, run: &Run, class: Class) -> Vec<Value> {
    let mut done: Vec<&crate::state::Intent> =
        run.intents.values().filter(|i| i.issuer == "model" && changes(class, &i.tool)).filter(|i| i.result.as_ref().is_some_and(|r| !r.is_error && r.status == "done")).collect();
    done.sort_by(|a, b| (a.at, &a.call_id).cmp(&(b.at, &b.call_id)));
    done.into_iter()
        .map(|i| {
            let dests = destinations_of(&i.input);
            let read_back = s.evidence.read_backs(&run.id).iter().find(|r| r.call_id == i.call_id).cloned();
            json!({ "call_id": i.call_id, "tool": i.tool, "what": what(class, &i.tool, &dests), "to": dests, "at": i.at, "read_back": read_back })
        })
        .collect()
}

/// What the agent tried that changes something and did not happen: refused
/// at the boundary, failed, or with an outcome nobody knows; and each
/// read-back that did not match what was done.
pub fn not_done(s: &State, run: &Run, class: Class) -> Vec<Value> {
    let tool_of =
        |call: &str| run.intents.values().find(|i| i.call_id == call).map(|i| i.tool.clone()).or_else(|| run.decisions.iter().find(|d| d.call_id == call).map(|d| d.tool.clone()));
    let mut failed: Vec<(i64, Value)> = run
        .results
        .iter()
        .filter(|(_, r)| r.is_error && matches!(r.status.as_str(), "denied" | "failed" | "unknown"))
        .filter_map(|(call, r)| {
            let tool = tool_of(call)?;
            let model = run.intents.values().find(|i| &i.call_id == call).is_none_or(|i| i.issuer == "model");
            (model && changes(class, &tool)).then(|| (r.seq, json!({ "call_id": call, "tool": tool, "status": r.status, "what": what(class, &tool, &[]) })))
        })
        .collect();
    failed.sort_by_key(|f| f.0);
    let mut out: Vec<Value> = failed.into_iter().map(|f| f.1).collect();
    for r in s.evidence.read_backs(&run.id).iter().filter(|r| r.matches == Some(false)) {
        out.push(json!({ "call_id": r.call_id, "tool": r.tool, "status": "read_back_differs", "what": r.note }));
    }
    out
}

fn changes(class: Class, tool: &str) -> bool {
    class(tool).is_some_and(|(app, risk)| risk.changes_state() && app != "memory" && risk != Risk::Prohibited)
}

/// A receipt's sentence: what was done and where, from the action's
/// structured fields alone, as an ask's card says it.
fn what(class: Class, tool: &str, dests: &[String]) -> String {
    let (app, risk) = class(tool).unwrap_or_else(|| (tool.split_once("__").map_or("", |(a, _)| a).to_string(), Risk::ReversibleWrite));
    let said = crate::asks::sentence(tool, &app, None, risk.as_str(), dests);
    let said = said.strip_prefix("Magnus wants to ").unwrap_or(&said).trim_end_matches('.');
    let mut c = said.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The addresses an action names, as its card would.
fn destinations_of(input: &Value) -> Vec<String> {
    let mut d: Vec<String> = crate::tools::addresses_in(&input.to_string()).into_iter().collect();
    if let Some(host) = input["url"].as_str().and_then(|u| url::Url::parse(u).ok()).and_then(|u| u.host_str().map(str::to_lowercase)) {
        d.push(host);
    }
    d.sort();
    d.dedup();
    d
}

/// The sources a result cites, checked against the record: each names a
/// call this run made that returned without error. What fails is said, so
/// the agent can fix it; nothing is cited on the agent's word alone.
pub fn cited(run: &Run, given: &Value) -> std::result::Result<Vec<Source>, String> {
    let Some(list) = given.as_array() else {
        return if given.is_null() { Ok(vec![]) } else { Err("sources is a list of {claim, call}".into()) };
    };
    if list.len() > MAX_SOURCES {
        return Err(format!("at most {MAX_SOURCES} sources; cite the ones the result rests on"));
    }
    let mut out = vec![];
    for (n, s) in list.iter().enumerate() {
        let claim = s["claim"].as_str().map(str::trim).unwrap_or_default();
        let call = s["call"].as_str().map(str::trim).unwrap_or_default();
        if claim.is_empty() || call.is_empty() {
            return Err(format!("source {}: it needs the claim and the call it came from", n + 1));
        }
        if claim.chars().count() > MAX_CLAIM_CHARS {
            return Err(format!("source {}: the claim is longer than {MAX_CLAIM_CHARS} characters; cite the figure or sentence itself", n + 1));
        }
        let ok = run.results.get(call).is_some_and(|r| !r.is_error);
        if !ok {
            return Err(format!("source {}: {call} is not a call of this task that returned a result; name the call ID a read gave", n + 1));
        }
        out.push(Source { claim: claim.into(), call: call.into() });
    }
    Ok(out)
}

/// Each cited source as the card links it: the claim, and the tool, the
/// page or file and the call its words came from.
pub fn sources_json(run: &Run, sources: &[Source]) -> Vec<Value> {
    sources
        .iter()
        .map(|s| {
            let i = run.intents.values().find(|i| i.call_id == s.call);
            let at = i.and_then(|i| i.input["url"].as_str().or(i.input["path"].as_str()).or(i.input["name"].as_str()).map(String::from));
            json!({ "claim": s.claim, "call_id": s.call, "tool": i.map(|i| i.tool.clone()), "at": at })
        })
        .collect()
}

/// The live line a task's card shows while it works: the plan's step when
/// it keeps one ("Reading the support inbox · step 2 of 4"), else what it is
/// doing now, from the last action it took.
pub fn status_line(run: &Run) -> Option<String> {
    if run.status.terminal() {
        return None;
    }
    if let Some(units) = run.plan.as_ref().and_then(|p| p["units"].as_array()).filter(|u| !u.is_empty()) {
        let at = units.iter().position(|u| u["status"] == "active").or_else(|| units.iter().position(|u| u["status"] != "done" && u["status"] != "dropped"));
        if let Some(n) = at {
            let goal = units[n]["goal"].as_str().unwrap_or_default().trim();
            return Some(format!("{goal} · step {} of {}", n + 1, units.len()));
        }
    }
    let last = run.intents.values().filter(|i| i.issuer == "model").max_by(|a, b| (a.at, &a.call_id).cmp(&(b.at, &b.call_id)))?;
    Some(doing(&last.tool))
}

/// What a tool call is doing, in a few plain words.
fn doing(tool: &str) -> String {
    let name = tool.split_once("__").map_or(tool, |(_, r)| r);
    let app = tool.split_once("__").map(|(a, _)| a);
    let verb = name.replace('_', " ");
    match (tool, app) {
        ("web_search", _) => "Searching the web".into(),
        ("web_fetch", _) | ("browser_navigate", _) | ("browser_snapshot", _) => "Reading a page".into(),
        (t, _) if t.starts_with("sandbox_") || t.starts_with("computer_") => "Working on its computer".into(),
        (_, Some(a)) => format!("{} in {a}", capitalised(&verb)),
        _ => capitalised(&verb),
    }
}

/// The first letter upper case, whatever its width in bytes.
fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl crate::kernel::Kernel {
    /// A run's evidence as its card shows it: receipts with their
    /// read-backs, what was not done, the sources cited, and while it works
    /// its status line.
    pub fn evidence_of(&self, run: &Run) -> Value {
        // Classed before the state is read: a connector's class is looked up outside it.
        let tools: std::collections::BTreeSet<String> = run.intents.values().map(|i| i.tool.clone()).chain(run.decisions.iter().map(|d| d.tool.clone())).collect();
        let classes: HashMap<String, (String, Risk)> = tools.into_iter().filter_map(|t| crate::boundary::facts(self, &t).map(|f| (t, (f.app, f.risk)))).collect();
        let class = |t: &str| classes.get(t).cloned();
        self.read(|s| {
            json!({
                "receipts": receipts(s, run, &class),
                "not_done": not_done(s, run, &class),
                "sources": sources_json(run, s.evidence.sources(&run.id)),
                "status_line": status_line(run),
            })
        })
    }
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
