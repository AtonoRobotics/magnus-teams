//! What still asks (spec rev 140, Permissions and tasks). Four things reach
//! the person, and nothing else does:
//!
//! 1. an action Auto Review judges outside what was asked (or cannot judge);
//! 2. a payment, every time;
//! 3. private data going to a new destination after the work read
//!    untrusted input, unless a rule covers it (invariant 4);
//! 4. anything the person's own rule says to ask first about.
//!
//! An ask is a card the client draws in the conversation the work came
//! from and in the approval list. The agent never writes or sees it: the
//! run only waits. Its sentence is the server's, made from structured
//! fields. Allow covers the rest of the task (same kind, same
//! destinations), Allow always saves a narrow rule, Don't allow stops the
//! action. A payment and case 3 are allowed one action at a time.
//!
//! Every approval card is made here, by `raise`, from a `Reason`: there is
//! no other way to put one in front of a person.
use crate::boundary::{is_chat_tool, shown_hash, Action, Facts, Gate, Issuer, Shown, Verdict};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::{Approval, Item, Run, State};
use crate::tools::Risk;
use anyhow::Result;
use serde_json::{json, Value};

/// Case 1's rule: Auto Review asked, or could not answer.
pub const OUTSIDE_REQUEST: &str = crate::auto_review::ASKED;
/// Case 3's rule: private data to a new destination after untrusted input.
pub const NEW_DESTINATION: &str = "kernel:rule_of_two";
/// Case 4 for the person's own computer set to ask before every action.
pub const LOCAL_ASK_FIRST: &str = "kernel:local_ask";
/// The most destinations a card's sentence names before it counts the rest.
const MAX_NAMED_DESTINATIONS: usize = 3;

/// Why a card asks: exactly the four cases, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    OutsideRequest,
    Payment,
    NewDestination,
    AskFirst,
}

impl Reason {
    pub const ALL: [Reason; 4] = [Reason::OutsideRequest, Reason::Payment, Reason::NewDestination, Reason::AskFirst];

    /// The reason an ask's rule names; None for anything that is not one of
    /// the four (an ask recorded before rev 140).
    pub fn of(rule: Option<&str>) -> Option<Reason> {
        let r = rule?;
        if r == OUTSIDE_REQUEST || r == crate::boundary::REVIEWER_FLAG {
            return Some(Reason::OutsideRequest);
        }
        if r == crate::payments::MONEY_RULE || r == crate::payments::RULE {
            return Some(Reason::Payment);
        }
        if r == NEW_DESTINATION {
            return Some(Reason::NewDestination);
        }
        if r == LOCAL_ASK_FIRST || r == crate::desktop::ASK_RULE || r.starts_with(crate::rule_effects::PERSON_RULE) {
            return Some(Reason::AskFirst);
        }
        None
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::OutsideRequest => "outside_request",
            Reason::Payment => "payment",
            Reason::NewDestination => "new_destination",
            Reason::AskFirst => "ask_first",
        }
    }

    /// Whether Allow may cover the rest of the task. A payment and private
    /// data to a new destination are allowed one action at a time.
    /// Spec, How an ask looks: "Allow covers the rest of the task". Not a
    /// payment, which asks every time.
    pub fn task_wide(self) -> bool {
        self != Reason::Payment
    }

    /// Whether Allow always is offered: wherever a rule can answer the ask
    /// (case 1, and case 3, which asks "unless a rule covers it"). Not for a
    /// payment, and not for the person's own ask-first rule, which wins over
    /// any allow rule.
    pub fn always(self) -> bool {
        self.rule_answers()
    }

    /// Whether an accepted allow rule answers this ask as the person's own
    /// approval. Case 3 asks unless a rule covers it; a payment always asks;
    /// ask first wins over allow.
    pub fn rule_answers(self) -> bool {
        matches!(self, Reason::OutsideRequest | Reason::NewDestination)
    }
}

/// A boundary action (spec rev 140, Boundary actions): a connector write, a
/// message to another person, a payment, a deletion outside the computer,
/// an install, a new destination for the computer, and a change to the
/// person's own desktop. Reads, and everything inside the computer, are not,
/// and pass without review. Only what the model asks for is one.
pub(crate) fn is_boundary(a: &Action, f: &Facts) -> bool {
    if a.issuer != Issuer::Model {
        return false;
    }
    if f.risk == Risk::Financial || a.tool == crate::rule_effects::SOURCE_TOOL {
        return true;
    }
    if !f.risk.changes_state() || is_chat_tool(f) || f.reaches_self {
        return false;
    }
    f.connector.is_some() || matches!(f.app.as_str(), "local" | "team" | "connector" | "web")
}

/// The host of a URL, as a destination is named.
fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(str::to_lowercase)
}

/// Where an action goes, as a card, an approval and a rule name it: the
/// recipients the gate read, a web lookup's host, a source's host.
pub(crate) fn destinations(a: &Action, f: &Facts, shown: Option<&Shown>) -> Vec<String> {
    if shown.is_none() && f.connector.is_none() && matches!(f.app.as_str(), "web" | "browser") {
        return a.input["url"].as_str().and_then(host_of).into_iter().collect();
    }
    let mut d = crate::rules::destinations_of(a, shown);
    d.sort();
    d.dedup();
    d
}

/// Whether an Allow given for `approved` covers an action going to `now`:
/// the same destinations, or fewer of them. An approval from before
/// destinations were kept covers as it did then.
pub fn same_destinations(approved: Option<&[String]>, now: &[String]) -> bool {
    match approved {
        None => true,
        Some(list) => list.is_empty() == now.is_empty() && now.iter().all(|d| list.iter().any(|x| x.eq_ignore_ascii_case(d))),
    }
}

/// One action, as an approval is matched against it.
pub struct Query<'a> {
    pub person: &'a str,
    /// The tasks whose approvals hold here: the run's own, its parents' for
    /// a sub-agent, and those of the chat turns in its conversation.
    pub tasks: &'a [String],
    pub tool: &'a str,
    pub risk: &'a str,
    pub hash: &'a str,
    pub reason: Option<Reason>,
    pub destinations: &'a [String],
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Whether one approval covers this action. Allow ("task") covers the rest
/// of its task for the same tool, risk and destinations (spec, How an ask
/// looks). Never a payment, covered only by the approval of that very
/// action; and case 3 only by an Allow the person gave on a case-3 card.
pub fn covers(ap: &Approval, q: &Query) -> bool {
    if ap.person != q.person || ap.tool != q.tool || ap.risk.as_deref() != Some(q.risk) {
        return false;
    }
    let given = Reason::of(ap.reason.as_deref());
    let same_case = q.reason != Some(Reason::NewDestination) || given == Some(Reason::NewDestination);
    let wide = same_case && q.reason.is_none_or(Reason::task_wide) && given.is_none_or(Reason::task_wide);
    match ap.scope.as_str() {
        "once" => !ap.used && q.tasks.contains(&ap.task) && ap.input_hash.as_deref() == Some(q.hash),
        "task" => wide && q.tasks.contains(&ap.task) && same_destinations(ap.destinations.as_deref(), q.destinations),
        // Written before rev 140, read as they were given.
        "window" => wide && ap.until.is_some_and(|u| u > q.now),
        "standing" => wide,
        _ => false,
    }
}

/// Whether the person already allowed private data to go to this
/// destination after untrusted input: an approved destination is remembered.
pub fn remembered(approvals: &[Approval], person: &str, destination: &str) -> bool {
    approvals.iter().any(|ap| {
        ap.person == person && Reason::of(ap.reason.as_deref()) == Some(Reason::NewDestination) && ap.destinations.iter().flatten().any(|d| d.eq_ignore_ascii_case(destination))
    })
}

/// The narrow rule Allow always saves: this person, this app and tool, these
/// destinations, allow automatically.
pub fn always_rule(person: &str, app: &str, tool: &str, destinations: &[String]) -> std::result::Result<crate::rules::Compiled, String> {
    crate::rules::compile(person, app, tool, destinations, false, crate::rules::Effect::Allow)
}

/// The plain name of where an action happens.
fn place(app: &str, title: Option<&str>) -> String {
    match (app, title) {
        (_, Some(t)) if !t.trim().is_empty() => t.trim().to_string(),
        ("local", _) => "your computer".into(),
        ("team", _) => "the team workspace".into(),
        ("web", _) => "the web".into(),
        ("payments", _) => "your payment method".into(),
        ("connector", _) => "Magnus".into(),
        (a, _) => {
            let mut c = a.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

/// "a, b and 2 more".
fn named(dests: &[String]) -> String {
    let shown: Vec<&str> = dests.iter().take(MAX_NAMED_DESTINATIONS).map(String::as_str).collect();
    let rest = dests.len().saturating_sub(MAX_NAMED_DESTINATIONS);
    match (shown.len(), rest) {
        (0, _) => String::new(),
        (1, 0) => shown[0].to_string(),
        (_, 0) => format!("{} and {}", shown[..shown.len() - 1].join(", "), shown[shown.len() - 1]),
        (_, n) => format!("{} and {n} more", shown.join(", ")),
    }
}

/// The card's one sentence: what Magnus wants to do and where, from the
/// action's structured fields alone. Nothing the model wrote is in it.
pub fn sentence(tool: &str, app: &str, title: Option<&str>, risk: &str, dests: &[String]) -> String {
    let to = named(dests);
    if tool == crate::rule_effects::SOURCE_TOOL {
        return format!("Magnus wants to let its computer reach {}.", if to.is_empty() { "a new site".into() } else { to });
    }
    let verb = tool.split_once("__").map_or(tool, |(_, r)| r).replace('_', " ");
    let verb = verb.trim();
    let at = place(app, title);
    if app == "web" && !to.is_empty() {
        return format!("Magnus wants to look up {to} on the web.");
    }
    let to = if to.is_empty() { String::new() } else { format!(", to {to}") };
    if risk == Risk::Financial.as_str() {
        return format!("Magnus wants to make a payment ({verb}) with {at}{to}.");
    }
    format!("Magnus wants to {verb} in {at}{to}.")
}

/// The card's sentence from an item's fields: a saved card's charge says
/// that it is the saved card, what it costs and where (`charge`, from
/// `saved_card_charge`); any other ask, what and where (`sentence`).
pub fn item_sentence(d: &Value, dests: &[String]) -> String {
    if let (Some(amount), true) = (d["charge"]["amount"].as_str(), d["charge"]["saved_card"] == true) {
        let at = d["charge"]["merchant"].as_str().filter(|m| !m.is_empty()).unwrap_or("the seller");
        return format!("Magnus wants to charge your saved card {amount} at {at}.");
    }
    sentence(d["tool"].as_str().unwrap_or_default(), d["app"].as_str().unwrap_or_default(), d["app_title"].as_str(), d["risk"].as_str().unwrap_or_default(), dests)
}

/// The fields of an approval item the card is made from, as a reader of
/// any item finds them (an item from before rev 140 lacks some).
pub fn item_destinations(d: &Value) -> Vec<String> {
    if let Some(a) = d["ask_destinations"].as_array() {
        return a.iter().filter_map(Value::as_str).map(String::from).collect();
    }
    let mut out: Vec<String> = d["destinations"]["recipients"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|v| v.as_array().cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    if out.is_empty() {
        out.extend(d["input"]["host"].as_str().map(String::from));
    }
    out
}

/// What an approval item's reasoning was: kept in the log for the developer
/// view and the run inspector, never shown on a user's surface.
pub const REASONING_FIELDS: [&str; 5] = ["why", "rule", "taint", "web_lookups", "reason"];

/// An ask's reasoning, for the developer view and the run inspector: which
/// of the four asks it is, the rule, the reason given and the run's taint.
pub fn reasoning(details: &Value) -> Value {
    json!({ "reason": details["reason"], "rule": details["rule"], "why": details["why"], "taint": details["taint"], "call_id": details["call_id"], "tool": details["tool"] })
}

/// What the waiting call is told when the person does not allow it: that
/// action stops, and the agent carries on with what does not depend on it.
pub fn declined(text: Option<&str>) -> String {
    format!(
        "The person did not allow this action{}. It was not done; do not try it again. Carry on with what does not depend on it.",
        text.map(|t| format!(": {t}")).unwrap_or_default()
    )
}

/// The card as the client draws it for `viewer`: one sentence, the exact
/// content one tap away (the item's input and destinations), and the
/// answers this viewer may give. Its reasoning stays out.
pub fn card(s: &State, viewer: &Person, i: &Item) -> Value {
    let d = &i.details;
    let reason = Reason::of(d["rule"].as_str());
    let dests = item_destinations(d);
    let say = item_sentence(d, &dests);
    let task_wide = reason.is_some_and(Reason::task_wide);
    let mut offers = vec!["allow"];
    if reason.is_some_and(Reason::always) && viewer.id == i.person && !dests.is_empty() && d["app"].as_str().is_some_and(|a| !a.is_empty()) {
        offers.push("allow_always");
    }
    offers.push("dont_allow");
    let thread = i.task.as_ref().and_then(|t| s.tasks.get(t)).and_then(|t| t.chat.as_ref()).map(|c| c.thread.clone());
    // Allowing a site is remembered for it (`remembered`): the card says so.
    let site = reason == Some(Reason::NewDestination) && !dests.is_empty() && matches!(d["app"].as_str(), Some("web" | "browser"));
    json!({
        "sentence": say,
        "destinations": dests,
        "offers": offers,
        "allow_scope": if task_wide { "task" } else { "once" },
        "allow_covers": if site { Some("site") } else { None },
        "conversation": thread,
    })
}

/// An item as a user's surface may carry it: the reasoning fields removed.
pub fn without_reasoning(details: &Value) -> Value {
    let mut d = details.clone();
    if let Some(m) = d.as_object_mut() {
        for k in REASONING_FIELDS {
            m.remove(k);
        }
    }
    d
}

/// What already answers an ask for the person.
pub(crate) enum Cover {
    Approval(Box<Approval>),
    Rule(String),
}

impl Kernel {
    /// The person's own answer to this action, if one holds: an approval
    /// that covers it, or (where a rule may answer) an accepted allow rule
    /// that matches it exactly. Either counts ahead of Auto Review.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn person_cover(&self, run: &Run, a: &Action, f: &Facts, shown: Option<&Shown>, hash: &str, dests: &[String], reason: Option<Reason>) -> Option<Cover> {
        let found: Vec<Approval> = self.read(|s| {
            // A sub-agent holds what was approved for each run above it, and
            // a chat turn what was approved in its conversation.
            let thread = |task: &str| s.tasks.get(task).and_then(|t| t.chat.as_ref()).filter(|c| c.turn).map(|c| c.thread.clone());
            let mut tasks = s.lineage(&run.id);
            let threads: Vec<String> = tasks.iter().filter_map(|t| thread(t)).collect();
            tasks.extend(s.approvals.iter().filter(|ap| thread(&ap.task).is_some_and(|t| threads.contains(&t))).map(|ap| ap.task.clone()));
            let q = Query { person: &run.person, tasks: &tasks, tool: &a.tool, risk: f.risk.as_str(), hash, reason, destinations: dests, now: chrono::Utc::now() };
            s.approvals.iter().filter(|ap| covers(ap, &q)).cloned().collect()
        });
        // An approval stands for the work only when the person whose work it
        // is gave it (spec rev 145, Approvals: an ask goes to the requester).
        // One an owner or approver gave for someone else, as before rev 140,
        // covers nothing.
        let theirs = |ap: &Approval| ap.by == format!("person:{}", run.person);
        if let Some(ap) = found.into_iter().find(theirs) {
            return Some(Cover::Approval(Box::new(ap)));
        }
        if f.risk == Risk::Financial || !reason.is_none_or(Reason::rule_answers) {
            return None;
        }
        // A rule stands for its person only while they could approve this
        // themselves.
        let may = self.person(&run.person).is_some_and(|p| p.is_owner() || p.has_role(&format!("approver:{}", f.risk.as_str())) || f.risk == Risk::ReversibleWrite);
        if !may {
            return None;
        }
        self.covering_rule(a, run, f, shown).map(Cover::Rule)
    }

    async fn record_cover(&self, a: &Action, cover: Cover, rule: Option<String>) -> Result<()> {
        match cover {
            Cover::Approval(ap) => {
                self.decision(a, "allow", "approval", &format!("approved by {} ({})", ap.by, ap.scope), rule).await?;
                if ap.scope == "once" {
                    self.emit("kernel", Ev::ApprovalUsed { id: ap.id.clone(), run: a.run.clone(), call_id: a.call_id.clone() }).await?;
                }
            }
            Cover::Rule(id) => {
                self.decision(a, "allow", "approval", &format!("covered by the person's rule {id}"), rule).await?;
                self.emit("kernel", Ev::Rule(crate::rules::Change::Used { id, run: a.run.clone(), call_id: a.call_id.clone() })).await?;
            }
        }
        Ok(())
    }

    /// After policy: the person's answer, then one of the four asks, then
    /// Auto Review on a boundary action. `asked` is the kernel's own ask
    /// (a payment, case 3, or the person's ask-first rule), with its why.
    pub(crate) async fn settle(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>, asked: Option<(String, String)>) -> Result<Gate> {
        let boundary = is_boundary(a, f);
        if asked.is_none() && !boundary {
            self.decision(a, "allow", "policy", "permitted; not a boundary action", None).await?;
            return Ok(Gate::Pass);
        }
        let hash = shown_hash(a, shown);
        let dests = self.destinations_of(a, f, shown);
        let reason = asked.as_ref().and_then(|(_, r)| Reason::of(Some(r)));
        if let Some(c) = self.person_cover(run, a, f, shown, &hash, &dests, reason) {
            self.record_cover(a, c, asked.map(|(_, r)| r)).await?;
            return Ok(Gate::Pass);
        }
        if let Some((why, rule)) = asked {
            let Some(reason) = reason else { return Err(anyhow::anyhow!("{rule} is not one of the four asks")) };
            return self.raise(a, run, f, shown, reason, &rule, &why).await;
        }
        // An enabled send reviewer judges first; its flag is case 1.
        if run.config.module("reviewer") && crate::boundary::is_reviewed_send(a, f) {
            if let Gate::Stop(v) = self.reviewer_stage(a, run, f, shown, &hash).await? {
                return Ok(Gate::Stop(v));
            }
        }
        // A grant or schedule the person set up answers for them, as an
        // Allow does, for an action that sends nothing out.
        let standing = match self.what_covers(run, f) {
            Some(s) if !crate::boundary::is_reviewed_send(a, f) => {
                self.record_standing(a, run, &None, s).await?;
                return Ok(Gate::Pass);
            }
            other => other,
        };
        let said = if standing.is_some() { "covered by a grant or schedule the person set up" } else { "asked for by the person" };
        match self.auto_review(a, f, run, shown, &hash, said).await? {
            crate::auto_review::Verdict::Allow(why) if self.auto_review_acts() || standing.is_some() => {
                match standing {
                    Some(s) => self.record_standing(a, run, &None, s).await?,
                    None => self.decision(a, "allow", "auto_review", &why, None).await?,
                }
                Ok(Gate::Pass)
            }
            crate::auto_review::Verdict::Allow(_) => {
                self.raise(a, run, f, shown, Reason::OutsideRequest, OUTSIDE_REQUEST, "the owner's policy does not let Auto Review decide this alone").await
            }
            crate::auto_review::Verdict::Deny(why) => Ok(Gate::Stop(self.deny(a, "auto_review", &why, false, None).await?)),
            crate::auto_review::Verdict::Ask(why) => self.raise(a, run, f, shown, Reason::OutsideRequest, OUTSIDE_REQUEST, &why).await,
        }
    }

    /// Put one of the four asks in front of the person, and let the run
    /// wait. Background work asks too, and waits as unwatched work does
    /// (`unattended`); only the team agent's own routines ask no one: the
    /// action is refused, and the agent may suggest it instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn raise(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>, reason: Reason, rule: &str, why: &str) -> Result<Gate> {
        if a.issuer != Issuer::Model {
            return Ok(Gate::Stop(self.deny(a, "approval", "only the agent's own actions wait for a person", false, Some(rule.into())).await?));
        }
        if self.read(|s| s.asks_no_one(&run.id)) {
            let text = format!("{why}. This is the team agent's own routine work, so no one is asked; if it is worth doing, report it to the channel and finish");
            return Ok(Gate::Stop(self.deny(a, "approval", &text, false, Some(rule.into())).await?));
        }
        self.decision(a, "ask", "approval", why, Some(rule.into())).await?;
        let open = self.read(|s| {
            s.items
                .values()
                .find(|i| i.run.as_deref() == Some(&a.run) && i.kind == "approval" && i.resolution.is_none() && i.details["call_id"] == a.call_id.as_str())
                .map(|i| i.id.clone())
        });
        let (id, item) = match open {
            Some(id) => (id, None),
            None => {
                let id = new_id("item");
                let dests = self.destinations_of(a, f, shown);
                let title = f.connector.as_ref().and_then(|c| c.title.clone());
                let details = json!({
                    "call_id": a.call_id, "tool": a.tool, "input": a.input, "input_hash": shown_hash(a, shown),
                    "risk": f.risk.as_str(), "app": f.app, "app_title": title, "audience": f.audience.as_str(),
                    "destinations": shown.map(Shown::details), "ask_destinations": dests,
                    "charge": self.saved_card_charge(a),
                    "reason": reason.as_str(), "why": why, "rule": rule,
                    "taint": { "untrusted": run.untrusted, "private": run.private },
                });
                let ev = Ev::Item {
                    id: id.clone(),
                    item_kind: "approval".into(),
                    task: Some(run.task.clone()),
                    run: Some(a.run.clone()),
                    person: run.person.clone(),
                    summary: item_sentence(&details, &dests),
                    details,
                };
                (id, Some(ev))
            }
        };
        self.suspend_on(&a.run, "approval", id, item).await?;
        Ok(Gate::Stop(Verdict::Suspended))
    }

    /// The send reviewer's judgment (#179): a pass lets the send on to Auto
    /// Review, a block denies it with its reason on the task, a flag is an
    /// ask of case 1.
    async fn reviewer_stage(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>, hash: &str) -> Result<Gate> {
        use crate::run::reviewer::SendVerdict;
        match self.review_send(run, &a.tool, &a.input, shown).await? {
            SendVerdict::Pass | SendVerdict::Unavailable => Ok(Gate::Pass),
            SendVerdict::Block(reason) => {
                let (_, ev) = crate::kernel::notice_event(
                    Some(run.task.clone()),
                    Some(a.run.clone()),
                    run.person.clone(),
                    format!(
                        "Magnus did not send this: {}.",
                        sentence(&a.tool, &f.app, f.connector.as_ref().and_then(|c| c.title.as_deref()), f.risk.as_str(), &self.destinations_of(a, f, shown)).trim_end_matches('.')
                    ),
                    json!({ "call_id": a.call_id, "tool": a.tool, "reason": reason }),
                );
                self.emit("kernel", ev).await?;
                Ok(Gate::Stop(self.deny(a, "reviewer", &crate::safety::blocked(&a.run, &a.tool, hash, &format!("the reviewer blocked this send: {reason}")), false, None).await?))
            }
            SendVerdict::Flag(reason) => {
                self.raise(a, run, f, shown, Reason::OutsideRequest, crate::boundary::REVIEWER_FLAG, &format!("the reviewer flagged this send: {reason}")).await
            }
        }
    }
}

/// A write that may have reached an app (cut off by a restart, or answered
/// with something unreadable), to an app that cannot tell a repeat from a
/// new request, is not sent again and asks no one (spec rev 140: a retry
/// never asks). The person is told, as a notice on the task, and the agent
/// is told to check before doing it again: the text returned.
pub(crate) async fn outcome_unknown(k: &Kernel, a: &Action, f: &Facts) -> Result<String> {
    let app = f.connector.as_ref().and_then(|c| c.title.clone()).unwrap_or_else(|| f.app.clone());
    if let Some(run) = k.read(|s| s.runs.get(&a.run).cloned()) {
        let summary = format!("{} may have reached {app}, and it was not sent again: check in {app} whether it happened.", a.tool);
        let (_, ev) = crate::kernel::notice_event(Some(run.task), Some(a.run.clone()), run.person, summary, json!({ "call_id": a.call_id, "tool": a.tool }));
        k.emit("kernel", ev).await?;
    }
    Ok(format!(
        "{} may have reached {app}, and {app} cannot tell a repeat from a new request, so it was not sent again. Its outcome is unknown: check in {app} before doing it again.",
        a.tool
    ))
}

#[cfg(test)]
#[path = "asks_tests.rs"]
mod tests;
