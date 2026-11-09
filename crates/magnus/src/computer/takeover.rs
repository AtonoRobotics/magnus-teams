//! A person taking the screen, and an operator looking with their leave.
//!
//! The controller is in the log. While a person holds the screen the agent
//! takes no step on that computer and its captures are refused. The display
//! itself is reached with a short-lived, single-use ticket carried in a
//! WebSocket subprotocol, never in a URL. Only the ticket's hash is kept.
use super::service::failed;
use super::{client, Activity, Change, Computer, Ledger, OperatorSession, Owner, Status};
use crate::config::Person;
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{Run, State};
use anyhow::{anyhow, Result};
use chrono::{Duration as Span, Utc};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TICKET_LIFETIME: Duration = Duration::from_secs(30);
const TICKET_BYTES: usize = 32;
const MAX_LIVE_TICKETS: usize = 1024;
pub(super) const TICKET_PROTOCOL_PREFIX: &str = "magnus.ticket.";
pub(super) const DISPLAY_PROTOCOL: &str = "magnus.display";
const SCREENSHOT_PATH: &str = "/tmp/magnus-shot.png";
const CAPTURE_TIMEOUT_SECONDS: u64 = 60;
const MAX_OPERATOR_MINUTES: i64 = 8 * 60;

struct Ticket {
    computer: String,
    person: String,
    issued: Instant,
}

static TICKETS: Mutex<Option<HashMap<String, Ticket>>> = Mutex::new(None);

fn digest(ticket: &str) -> String {
    hex::encode(Sha256::digest(ticket.as_bytes()))
}

fn issue(computer: &str, person: &str) -> String {
    let mut bytes = [0u8; TICKET_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    let ticket = hex::encode(bytes);
    let mut slot = TICKETS.lock().unwrap_or_else(|e| e.into_inner());
    let map = slot.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_LIVE_TICKETS {
        map.retain(|_, t| t.issued.elapsed() < TICKET_LIFETIME);
    }
    map.insert(digest(&ticket), Ticket { computer: computer.to_string(), person: person.to_string(), issued: Instant::now() });
    ticket
}

/// The person a ticket was issued to. A presented ticket is spent, whatever
/// else is wrong with it.
pub(super) fn redeem(raw: &str, computer: &str) -> Option<String> {
    let mut slot = TICKETS.lock().unwrap_or_else(|e| e.into_inner());
    let t = slot.get_or_insert_with(HashMap::new).remove(&digest(raw))?;
    (t.computer == computer && t.issued.elapsed() < TICKET_LIFETIME).then_some(t.person)
}

pub(super) fn apply(l: &mut Ledger, change: &Change) {
    let note = |what: String, who: &str, at| Activity { what, who: who.to_string(), at };
    match change {
        Change::TakeoverStarted { id, by, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                c.controller = Some(by.clone());
                c.activity.push(note("took control".into(), by, *at));
            }
        }
        Change::TakeoverEnded { id, by, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                c.controller = None;
                c.activity.push(note("handed back".into(), by, *at));
            }
        }
        Change::OperatorGranted { id, session, operator, until, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                c.operators.push(OperatorSession { session: session.clone(), operator: operator.clone(), until: *until, ended: false });
                c.activity.push(note(format!("let an operator look until {until}"), operator, *at));
            }
        }
        Change::OperatorEnded { id, session, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                let mut who = String::new();
                for s in c.operators.iter_mut().filter(|s| s.session == *session) {
                    s.ended = true;
                    who = s.operator.clone();
                }
                c.activity.push(note("ended an operator's session".into(), &who, *at));
            }
        }
        Change::OperatorLooked { id, operator, what, at, .. } => {
            if let Some(c) = l.computers.get_mut(id) {
                c.activity.push(note(format!("an operator read {what}"), operator, *at));
            }
        }
        _ => {}
    }
}

impl State {
    pub(crate) fn add_frame(&mut self, run: &str, step: u32, image_b64: &str) {
        if let Some(r) = self.runs.get_mut(run).filter(|_| !image_b64.is_empty()) {
            r.frames.push((step, image_b64.to_string()));
        }
    }
}

/// Whose it is to use: the person it belongs to. An operator, admin or owner
/// reaches its screen and files only through a session its user granted.
fn may_open(who: &Person, owner: &Owner) -> bool {
    match owner {
        Owner::Person(p) => who.id == *p,
        Owner::Workspace(_) => who.is_owner() || who.has_role("admin"),
    }
}

/// The model's desktop tools, on the person's own computer.
pub(super) fn is_capture(tool: &str) -> bool {
    tool.starts_with("computer_")
}

impl Kernel {
    pub(super) fn opened_computer(&self, who: &Person, id: &str) -> Result<Computer, Refused> {
        self.read(|s| s.computers.get(id).cloned()).filter(|c| c.role == super::Role::Computer && may_open(who, &c.owner)).ok_or(Refused::NotFound("computer"))
    }

    /// The running computer a person's run acts on, if they have one.
    pub(crate) fn run_computer(&self, person: &str) -> Option<String> {
        self.read(|s| {
            s.computers.all().find(|c| c.role == super::Role::Computer && c.status == Status::Running && c.owner == Owner::Person(person.to_string())).map(|c| c.id.clone())
        })
    }

    /// Whether a person holds the screen of the computer this run is using.
    pub(crate) fn computer_held(&self, run: &Run) -> bool {
        let uses = run.replies.iter().any(|r| r.calls().iter().any(|(_, name, _)| is_capture(name)));
        uses && self.read(|s| s.computers.all().any(|c| c.owner == Owner::Person(run.person.clone()) && c.controller.is_some()))
    }

    async fn capture(&self, socket: &Path, id: &str) -> Result<String> {
        self.act_and_capture(socket, id, "computer_screenshot", &json!({})).await
    }

    async fn act_and_capture(&self, socket: &Path, id: &str, tool: &str, input: &Value) -> Result<String> {
        let command = crate::boundary::computer_command(tool, input)?;
        let done = client::guest(socket, id, json!({ "op": "exec", "command": command, "timeout_s": CAPTURE_TIMEOUT_SECONDS })).await?;
        if done["exit_code"] != json!(0) {
            return Err(anyhow!("the desktop action failed: {}", done["stderr"].as_str().unwrap_or_default()));
        }
        let shot = client::guest(socket, id, json!({ "op": "read", "path": SCREENSHOT_PATH })).await?;
        shot["data_b64"].as_str().map(String::from).ok_or_else(|| anyhow!("the computer did not return its screen"))
    }

    /// A desktop tool on the person's computer. `None`: they have none
    /// running, and it goes to the run's own sandbox.
    pub(crate) async fn computer_tool(&self, person: &str, tool: &str, input: &Value) -> Result<Option<(String, bool, Option<Value>)>> {
        let Some(id) = self.run_computer(person) else { return Ok(None) };
        let held = self.read(|s| s.computers.get(&id).is_some_and(|c| c.controller.is_some()));
        if held {
            return Ok(Some(("A person has control of this computer, so this was refused. Wait until they hand it back.".to_string(), true, None)));
        }
        super::idle::touch(&id);
        let socket = self.computer_socket().map_err(|e| anyhow!("{e}"))?;
        let image = self.act_and_capture(&socket, &id, tool, input).await?;
        Ok(Some(("Screenshot of your computer attached.".to_string(), false, Some(json!({ "image_b64": image })))))
    }

    /// A boundary action: the pipeline has already reviewed it by now.
    pub(crate) async fn add_destination_tool(&self, a: &crate::boundary::Action) -> Result<(String, bool, Option<Value>)> {
        let person = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone())).and_then(|p| self.person(&p)).ok_or_else(|| anyhow!("the run has no person"))?;
        let Some(id) = self.run_computer(&person.id) else { return Ok(("You have no running computer.".to_string(), true, None)) };
        let host = a.input["host"].as_str().unwrap_or_default();
        Ok(match self.add_computer_destination(&person, &id, host).await {
            Ok(_) => (format!("{host} can now be reached from your computer."), false, None),
            Err(e) => (e.to_string(), true, None),
        })
    }

    pub async fn start_takeover(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("only a running computer can be taken over".into()));
        }
        // The user always wins: a new holder simply replaces the last.
        if c.controller.as_deref() != Some(who.id.as_str()) {
            self.record_computer(who, Change::TakeoverStarted { id: id.into(), by: who.id.clone(), at: Utc::now() }).await?;
        }
        super::idle::touch(id);
        Ok(json!({ "ticket": issue(id, &who.id), "protocol_prefix": TICKET_PROTOCOL_PREFIX, "expires_in_seconds": TICKET_LIFETIME.as_secs() }))
    }

    /// "I'm done": the agent picks up with a fresh screenshot.
    pub async fn hand_back(self: &Arc<Self>, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        if c.controller.is_none() {
            return Err(Refused::Conflict("no one holds this computer".into()));
        }
        let Owner::Person(owner) = c.owner.clone() else { return Err(Refused::Denied("a workspace computer has no agent to hand back to yet".into())) };
        let socket = self.computer_socket()?;
        let frame = self.capture(&socket, id).await.ok();
        let runs: Vec<(String, u32)> = self.read(|s| {
            s.runs
                .values()
                .filter(|r| r.person == owner && r.status == crate::state::Status::Suspended && r.reason.as_deref() == Some("takeover"))
                .map(|r| (r.id.clone(), r.replies.len() as u32))
                .collect()
        });
        let actor = format!("person:{}", who.id);
        let mut evs: Vec<(&str, Ev)> = vec![(actor.as_str(), Ev::Computer(Change::TakeoverEnded { id: id.into(), by: who.id.clone(), at: Utc::now() }))];
        for (run, step) in &runs {
            evs.push((
                "kernel",
                Ev::Notice {
                    run: run.clone(),
                    step: *step,
                    text: "A person took control of your computer and has handed it back. Things may have changed: rely on this fresh screenshot, not on what you saw before."
                        .to_string(),
                },
            ));
            if let Some(image_b64) = frame.clone() {
                evs.push(("kernel", Ev::Handback { run: run.clone(), step: *step, image_b64 }));
            }
            evs.extend(self.resume(run));
        }
        self.emit_all(evs).await.map_err(failed)?;
        for (run, _) in &runs {
            self.wake(run);
        }
        Ok(json!({ "controller": Value::Null, "resumed": runs.len() }))
    }

    pub async fn grant_operator(&self, who: &Person, id: &str, operator: &str, minutes: i64) -> Result<Value, Refused> {
        self.opened_computer(who, id)?;
        if !(1..=MAX_OPERATOR_MINUTES).contains(&minutes) {
            return Err(Refused::Invalid(format!("a session is 1 to {MAX_OPERATOR_MINUTES} minutes")));
        }
        if self.person(operator).is_none() {
            return Err(Refused::Invalid("no such operator".into()));
        }
        let session = crate::kernel::new_id("oper");
        let now = Utc::now();
        self.record_computer(who, Change::OperatorGranted { id: id.into(), session: session.clone(), operator: operator.into(), until: now + Span::minutes(minutes), at: now })
            .await?;
        Ok(json!({ "session": session }))
    }

    pub async fn end_operator(&self, who: &Person, id: &str, session: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        if !c.operators.iter().any(|s| s.session == session && !s.ended) {
            return Err(Refused::NotFound("session"));
        }
        self.record_computer(who, Change::OperatorEnded { id: id.into(), session: session.into(), at: Utc::now() }).await?;
        Ok(json!({ "ended": session }))
    }

    pub fn computer_activity(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        Ok(json!({ "activity": self.opened_computer(who, id)?.activity }))
    }

    /// Records the look before anything is shown; no session, no look.
    async fn operator_session(&self, who: &Person, id: &str, what: &str) -> Result<Computer, Refused> {
        let now = Utc::now();
        let c = self.read(|s| s.computers.get(id).cloned()).ok_or(Refused::NotFound("computer"))?;
        let Some(session) = c.operators.iter().find(|s| s.operator == who.id && !s.ended && s.until > now).map(|s| s.session.clone()) else {
            return Err(Refused::Denied("this computer's user has not granted you a session".into()));
        };
        self.record_computer(who, Change::OperatorLooked { id: id.into(), session, operator: who.id.clone(), what: what.into(), at: now }).await?;
        Ok(c)
    }

    pub async fn operator_file(&self, who: &Person, id: &str, path: &str) -> Result<Value, Refused> {
        let c = self.operator_session(who, id, &format!("the file {path}")).await?;
        self.computer_file_for(&c, json!({ "op": "read", "path": path })).await
    }

    pub async fn operator_screen(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.operator_session(who, id, "the screen").await?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("the computer is not running".into()));
        }
        let socket = self.computer_socket()?;
        let image = self.capture(&socket, id).await.map_err(failed)?;
        Ok(json!({ "image_b64": image }))
    }

    /// The controller's display stream, once the ticket has been redeemed.
    pub(super) async fn display_stream(&self, id: &str, person: &str) -> Result<tokio::net::UnixStream, Refused> {
        let holds = self.read(|s| s.computers.get(id).is_some_and(|c| c.controller.as_deref() == Some(person) && c.status == Status::Running));
        if !holds {
            return Err(Refused::Denied("you do not hold this computer".into()));
        }
        let socket = self.computer_socket()?;
        client::display(&socket, id).await.map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_works_once_and_only_for_its_computer() {
        let t = issue("a", "alice");
        assert_eq!(redeem(&t, "a"), Some("alice".to_string()));
        assert_eq!(redeem(&t, "a"), None, "a ticket worked twice");
        let other = issue("a", "alice");
        assert_eq!(redeem(&other, "b"), None, "a ticket opened another computer");
        assert_eq!(redeem(&other, "a"), None, "a misused ticket was not spent");
    }

    #[test]
    fn an_old_ticket_is_refused() {
        let t = issue("a", "alice");
        {
            let mut slot = TICKETS.lock().unwrap();
            let entry = slot.as_mut().unwrap().get_mut(&digest(&t)).unwrap();
            entry.issued = Instant::now().checked_sub(TICKET_LIFETIME + Duration::from_secs(1)).unwrap();
        }
        assert_eq!(redeem(&t, "a"), None);
    }

    #[test]
    fn only_a_person_opens_their_own_computer() {
        let p = |id: &str, roles: serde_json::Value| -> Person { serde_json::from_value(json!({ "id": id, "groups": [], "roles": roles })).unwrap() };
        let owner = Owner::Person("alice".into());
        assert!(may_open(&p("alice", json!([])), &owner));
        assert!(!may_open(&p("root", json!(["owner", "admin"])), &owner), "an admin opened a person's computer without a session");
    }
}
