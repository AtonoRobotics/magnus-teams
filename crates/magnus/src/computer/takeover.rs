//! A person taking the screen, and an operator looking with their leave.
//!
//! The controller is in the log. While a person holds the screen the agent
//! takes no step on that computer and its captures are refused. The display
//! itself is reached with a short-lived, single-use ticket carried in a
//! WebSocket subprotocol, never in a URL. Only the ticket's hash is kept.
use super::leftovers;
use super::service::failed;
use super::wake::{act_and_capture, screenshot, Woken};
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TICKET_LIFETIME: Duration = Duration::from_secs(30);
const TICKET_BYTES: usize = 32;
const MAX_LIVE_TICKETS: usize = 1024;
pub(super) const TICKET_PROTOCOL_PREFIX: &str = "magnus.ticket.";
pub(super) const DISPLAY_PROTOCOL: &str = "magnus.display";
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

/// The runs that use a computer with this owner.
pub(super) fn runs_on<'a>(s: &'a crate::state::State, owner: &'a Owner) -> impl Iterator<Item = &'a crate::state::Run> {
    s.runs.values().filter(move |r| match owner {
        Owner::Person(p) => r.person == *p && s.team.run_of(&r.task).is_none(),
        Owner::Workspace(w) => s.team.run_of(&r.task).is_some_and(|t| t.workspace == *w),
    })
}

pub(super) fn apply(l: &mut Ledger, change: &Change) {
    let note = |what: String, who: &str, at| Activity { what, who: who.to_string(), at };
    match change {
        Change::TakeoverStarted { id, by, at, cause, runs } => {
            l.takeovers.push((cause.clone(), runs.clone()));
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
        Change::TakeoverCleared { id, by, stopped, stopped_count, removed, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                let cleared = leftovers::Cleared { stopped: stopped.clone(), stopped_count: *stopped_count, removed: removed.clone() };
                c.activity.push(note(leftovers::activity(&cleared), by, *at));
            }
        }
        Change::TakeoverUnclean { id, by, why, at } => {
            if let Some(c) = l.computers.get_mut(id) {
                c.activity.push(note(format!("could not confirm the takeover left nothing running ({why}); the agent stays paused"), by, *at));
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
/// reaches its screen and files only through a session its user granted. A
/// workspace's is its members', and a server admin outside it is not one.
fn may_open(s: &crate::state::State, who: &Person, owner: &Owner) -> bool {
    match owner {
        Owner::Person(p) => who.id == *p,
        Owner::Workspace(w) => s.team.role(w, &who.id).is_some_and(|r| r >= crate::team::Role::Member),
    }
}

/// The model's desktop tools, on the person's own computer.
pub(super) fn is_capture(tool: &str) -> bool {
    tool.starts_with("computer_")
}

impl Kernel {
    pub(super) fn opened_computer(&self, who: &Person, id: &str) -> Result<Computer, Refused> {
        self.read(|s| s.computers.get(id).filter(|c| c.role == super::Role::Computer && may_open(s, who, &c.owner)).cloned()).ok_or(Refused::NotFound("computer"))
    }

    /// The workspace's one computer: awake, or asleep and woken by its use.
    pub(crate) fn team_computer(&self, workspace: &str) -> Option<String> {
        let owner = Owner::Workspace(workspace.to_string());
        self.read(|s| s.computers.one_of(&owner).map(|c| c.id.clone()))
    }

    /// Whose computer a run acts on: its team's on a team request, its person's otherwise.
    pub(crate) fn owner_of_run(&self, run: &Run) -> Owner {
        match self.team_of(&run.task) {
            Some(t) => Owner::Workspace(t.workspace),
            None => Owner::Person(run.person.clone()),
        }
    }

    /// The computer a run acts on.
    pub(crate) fn computer_of_run(&self, run: &Run) -> Option<String> {
        let owner = self.owner_of_run(run);
        self.read(|s| s.computers.one_of(&owner).map(|c| c.id.clone()))
    }

    /// Why this run must wait for a person before going on, if it must.
    pub(crate) fn waits_on_person(&self, run: &Run) -> Option<&'static str> {
        if self.computer_held(run) {
            return Some("takeover");
        }
        if self.read(|s| super::driving::open_request(s, &run.id).is_some()) {
            return Some(super::driving::REQUEST_REASON);
        }
        let form = |i: &crate::state::Item| i.kind == "secure_form" && i.resolution.is_none() && i.run.as_deref() == Some(run.id.as_str());
        self.read(|s| s.items.values().any(form)).then_some("secure_form")
    }

    /// Whether a person holds the screen of the computer this run is using.
    pub(crate) fn computer_held(&self, run: &Run) -> bool {
        let uses = run.replies.iter().any(|r| r.calls().iter().any(|(_, name, _)| is_capture(name)));
        let mine = self.computer_of_run(run);
        uses && self.read(|s| s.computers.all().any(|c| Some(&c.id) == mine.as_ref() && c.controller.is_some()))
    }

    /// A desktop tool on the run's one computer: made if it has none yet,
    /// woken if it sleeps, so a run never finds itself without one (spec
    /// rev 142). `None` only on a server without computers: it goes to the
    /// run's own sandbox.
    pub(crate) async fn computer_tool(&self, run: &str, tool: &str, input: &Value) -> Result<Option<(String, bool, Option<Value>)>> {
        if tool == super::driving::REQUEST_TOOL {
            return self.request_takeover(run, input).await.map(Some);
        }
        let Some(owner) = self.read(|s| s.runs.get(run).cloned()).map(|r| self.owner_of_run(&r)) else { return Ok(None) };
        if super::agent_tools::handles(tool) {
            return self.agent_computer_tool(&owner, &format!("run:{run}"), tool, input).await;
        }
        self.use_computer(&owner, &format!("run:{run}"), tool, input).await
    }

    /// One desktop tool on `owner`'s computer, for `actor`.
    pub(crate) async fn use_computer(&self, owner: &Owner, actor: &str, tool: &str, input: &Value) -> Result<Option<(String, bool, Option<Value>)>> {
        let id = match self.computer_for(owner).await {
            Ok(Some(id)) => id,
            Ok(None) => return Ok(None),
            Err(e) => return Ok(Some((format!("Your computer could not be made ready: {e}"), true, None))),
        };
        let held = self.read(|s| s.computers.get(&id).is_some_and(|c| c.controller.is_some()));
        if held {
            return Ok(Some(("A person has control of this computer, so this was refused. Wait until they hand it back.".to_string(), true, None)));
        }
        let room = std::time::Duration::from_secs(super::wake::ROOM_WAIT_MINUTES * 60);
        if let Err(e) = self.wake_in_turn(&id, actor, room).await {
            return Ok(Some((format!("Your computer could not wake: {e}"), true, None)));
        }
        super::idle::touch(&id);
        let socket = self.computer_socket().map_err(|e| anyhow!("{e}"))?;
        let image = act_and_capture(&socket, &id, tool, input).await?;
        Ok(Some(("Screenshot of your computer attached.".to_string(), false, Some(json!({ "image_b64": image })))))
    }

    /// Taking control wakes a sleeping computer first.
    pub async fn start_takeover(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        self.opened_computer(who, id)?;
        if let Woken::Queued(n) = self.wake_for_use(id, &format!("person:{}", who.id)).await? {
            return Err(Refused::Conflict(format!("the computer is waiting for room on the server, number {n} in line")));
        }
        let c = self.opened_computer(who, id)?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("only a running computer can be taken over".into()));
        }
        // The user always wins: a new holder simply replaces the last, and
        // keeps the boundary the first one's session began at.
        if c.controller.is_none() {
            let socket = self.computer_socket()?;
            leftovers::begin(&socket, id).await.map_err(|e| Refused::Conflict(leftovers::begin_refused(&e)))?;
        }
        if c.controller.as_deref() != Some(who.id.as_str()) {
            let (cause, runs) = self.read(|s| {
                let on_it = runs_on(s, &c.owner).filter(|r| !r.status.terminal());
                let (waiting, active): (Vec<_>, Vec<_>) = on_it.partition(|r| r.status == crate::state::Status::Suspended);
                let asked = waiting.iter().find_map(|r| s.items.values().find(|i| i.run.as_deref() == Some(r.id.as_str()) && i.resolution.is_none()));
                let cause = match (asked, waiting.is_empty() && active.is_empty()) {
                    (Some(item), _) => super::driving::cause_of(item),
                    (None, true) => "nothing was running".to_string(),
                    (None, false) => "the person stepped in".to_string(),
                };
                (cause, waiting.iter().chain(&active).map(|r| r.id.clone()).collect())
            });
            self.record_computer(who, Change::TakeoverStarted { id: id.into(), by: who.id.clone(), at: Utc::now(), cause: Some(cause), runs }).await?;
        }
        super::idle::touch(id);
        Ok(json!({ "ticket": issue(id, &who.id), "protocol_prefix": TICKET_PROTOCOL_PREFIX, "expires_in_seconds": TICKET_LIFETIME.as_secs() }))
    }

    /// "I'm done": once the guest has stopped what the session left running,
    /// the agent picks up with a fresh screenshot. Until it confirms that,
    /// the person keeps the screen and the agent stays paused.
    pub async fn hand_back(self: &Arc<Self>, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        if c.controller.is_none() {
            return Err(Refused::Conflict("no one holds this computer".into()));
        }
        let socket = self.computer_socket()?;
        let _closing = leftovers::handing_back(id);
        super::display::cut(id);
        let cleared = match leftovers::end(&socket, id).await {
            Ok(cleared) => cleared,
            Err(why) => {
                self.record_computer(who, Change::TakeoverUnclean { id: id.into(), by: who.id.clone(), why: why.clone(), at: Utc::now() }).await?;
                return Err(Refused::Conflict(leftovers::unclean(&why)));
            }
        };
        let frame = screenshot(&socket, id).await.ok();
        let runs: Vec<(String, u32)> = self.read(|s| {
            runs_on(s, &c.owner)
                .filter(|r| r.status == crate::state::Status::Suspended && matches!(r.reason.as_deref(), Some("takeover" | super::driving::REQUEST_REASON)))
                .map(|r| (r.id.clone(), r.replies.len() as u32))
                .collect()
        });
        let ids: Vec<String> = runs.iter().map(|(r, _)| r.clone()).collect();
        let answered = self.read(|s| super::driving::answered_by_handback(s, id, &ids));
        let actor = format!("person:{}", who.id);
        let now = Utc::now();
        let report = json!({ "stopped": cleared.stopped, "stopped_count": cleared.stopped_count, "removed": cleared.removed });
        let leftover =
            Change::TakeoverCleared { id: id.into(), by: who.id.clone(), stopped: cleared.stopped, stopped_count: cleared.stopped_count, removed: cleared.removed, at: now };
        let mut evs: Vec<(&str, Ev)> =
            vec![(actor.as_str(), Ev::Computer(leftover)), (actor.as_str(), Ev::Computer(Change::TakeoverEnded { id: id.into(), by: who.id.clone(), at: now }))];
        // What the agent asked for is answered by handing back.
        evs.extend(answered.into_iter().map(|item| (actor.as_str(), Ev::ItemResolved { id: item, decision: "handed_back".into(), scope: None, text: None })));
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
        Ok(json!({ "controller": Value::Null, "resumed": runs.len(), "left_running": report }))
    }

    /// What "I'm done" would close, for the person holding the screen to
    /// save their work first. A computer that cannot say is unclean at
    /// handback anyway, so the error is the person's to see.
    pub async fn handback_closes(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        if c.controller.as_deref() != Some(who.id.as_str()) {
            return Err(Refused::Conflict("you do not hold this computer".into()));
        }
        let socket = self.computer_socket()?;
        let programs = leftovers::preview(&socket, id).await.map_err(failed)?;
        Ok(json!({ "programs": programs }))
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

    /// A session reaches the user's files, as they do, and nothing else of
    /// the computer: not its home, its browser profiles or their cookies.
    pub async fn operator_file(&self, who: &Person, id: &str, path: &str) -> Result<Value, Refused> {
        let path = super::service::user_path(path)?;
        let c = self.operator_session(who, id, &format!("the file {path}")).await?;
        self.computer_file_for(&c, json!({ "op": "read", "path": path })).await
    }

    /// A look at the screen, for the person. It is no use of the computer,
    /// so it neither keeps it awake nor wakes it. While it sleeps or wakes
    /// the answer is its last screenshot and how far the wake has got.
    pub async fn computer_preview(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id)?;
        let waking = super::wake::waking(id);
        if c.status != Status::Running || waking.is_some() {
            let (image, at) = super::wake::last_screen(id).map_or((None, None), |(i, at)| (Some(i), Some(at)));
            return Ok(json!({ "image_b64": image, "at": at, "status": c.status, "waking": waking, "last": true }));
        }
        let socket = self.computer_socket()?;
        let image = screenshot(&socket, id).await.map_err(failed)?;
        Ok(json!({ "image_b64": image, "status": c.status }))
    }

    pub async fn operator_screen(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.operator_session(who, id, "the screen").await?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("the computer is not running".into()));
        }
        let socket = self.computer_socket()?;
        let image = screenshot(&socket, id).await.map_err(failed)?;
        Ok(json!({ "image_b64": image }))
    }

    /// The controller's display stream, once the ticket has been redeemed.
    pub(super) async fn display_stream(&self, id: &str, person: &str) -> Result<tokio::net::UnixStream, Refused> {
        let holds = self.read(|s| s.computers.get(id).is_some_and(|c| c.controller.as_deref() == Some(person) && c.status == Status::Running));
        if !holds || leftovers::is_handing_back(id) {
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
        let s = crate::state::State::default();
        assert!(may_open(&s, &p("alice", json!([])), &owner));
        assert!(!may_open(&s, &p("root", json!(["owner", "admin"])), &owner), "an admin opened a person's computer without a session");
        assert!(!may_open(&s, &p("root", json!(["owner", "admin"])), &Owner::Workspace("w".into())), "a server admin outside a workspace opened its computer");
    }
}
