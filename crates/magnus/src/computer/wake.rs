//! Any use wakes a computer (spec, Computer state: Lifecycle). The agent's
//! desktop tools, a schedule's or a trigger's run, and the person opening its
//! screen or files or taking control all wake it; nobody starts it by hand.
//! While it wakes the client shows the last screenshot and the progress, so
//! the last screenshot of each computer is kept here, taken before it sleeps.
//!
//! The paused state is the kill switch: no use wakes it. Only its person's
//! Resume does (`decisions/2026-11-09-wake-on-use.md`).
use super::{client, Change, Status};
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// As long as a wake may take before a use gives up on it: a cold boot,
/// then putting the computer's apps back.
const WAKE_WAIT: Duration = Duration::from_secs(5 * 60);
/// How often a use waiting on another's wake looks again.
const WAKE_POLL: Duration = Duration::from_millis(250);

/// What a use does with a computer in this state.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Plan {
    Ready,
    Wake,
    /// In the queue for room on the server; the queue wakes it.
    Waiting,
    Refused(&'static str),
}

pub(super) fn plan(status: Status) -> Plan {
    match status {
        Status::Running => Plan::Ready,
        // A destroyed computer that kept its files is rebuilt on its next use.
        Status::Stopped | Status::Hibernated | Status::Killed => Plan::Wake,
        Status::Queued => Plan::Waiting,
        Status::Paused => Plan::Refused("the computer is paused by its kill switch. Its person resumes it."),
    }
}

/// Where a wake has got to, in the order it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Starting,
    Booting,
    Restoring,
}

impl Phase {
    fn word(self) -> &'static str {
        match self {
            Phase::Starting => "starting",
            Phase::Booting => "booting",
            Phase::Restoring => "restoring",
        }
    }
}

/// Per computer: something and since when.
type Since<T> = Mutex<Option<HashMap<String, (T, DateTime<Utc>)>>>;

static WAKING: Since<Phase> = Mutex::new(None);
static LAST_SCREEN: Since<String> = Mutex::new(None);

/// Takes the wake of `id` for this caller; false when another already has it.
pub(super) fn claim(id: &str) -> bool {
    let mut slot = WAKING.lock().unwrap_or_else(|e| e.into_inner());
    let map = slot.get_or_insert_with(HashMap::new);
    if map.contains_key(id) {
        return false;
    }
    map.insert(id.to_string(), (Phase::Starting, Utc::now()));
    true
}

pub(super) fn set_phase(id: &str, phase: Phase) {
    if let Some((p, _)) = WAKING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get_mut(id) {
        *p = phase;
    }
}

pub(super) fn release(id: &str) {
    WAKING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(id);
}

/// The wake under way, as the client shows it.
pub(super) fn waking(id: &str) -> Option<Value> {
    let slot = WAKING.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_ref()?.get(id).map(|(p, since)| json!({ "phase": p.word(), "since": since }))
}

/// Ends the claim however the wake ends.
struct Claimed<'a>(&'a str);

impl Drop for Claimed<'_> {
    fn drop(&mut self) {
        release(self.0);
    }
}

/// Keeps the newest picture of a computer's screen, in memory only: a
/// screenshot can show anything, and the host keeps none at rest.
pub(super) fn keep_screen(id: &str, image_b64: &str) {
    if image_b64.is_empty() {
        return;
    }
    LAST_SCREEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id.to_string(), (image_b64.to_string(), Utc::now()));
}

pub(super) fn last_screen(id: &str) -> Option<(String, DateTime<Utc>)> {
    LAST_SCREEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(id).cloned()
}

/// A deleted computer's picture goes with it.
pub(super) fn forget_screen(id: &str) {
    LAST_SCREEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(id);
}

/// The VM side of a wake: computerd wakes it, and the guest answers.
pub(super) enum Booted {
    Woke(Value),
    AtCapacity,
}

/// Asks computerd to wake `id`; `started` runs as soon as the VM is up, before
/// the guest answers, so the log has it running while it boots.
pub(super) async fn boot<F, Fut>(socket: &Path, id: &str, started: F) -> Result<Booted>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    set_phase(id, Phase::Starting);
    let woke = match client::ask(socket, json!({ "op": "wake", "id": id })).await {
        Ok(v) => v,
        Err(e) if client::at_capacity(&e) => return Ok(Booted::AtCapacity),
        Err(e) => return Err(e),
    };
    started().await?;
    set_phase(id, Phase::Booting);
    super::service::wait_for_guest(socket, id).await?;
    Ok(Booted::Woke(woke))
}

/// What a use found once its wake was done.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Woken {
    Ready,
    Queued(usize),
}

impl Kernel {
    /// Wakes `id` if it sleeps and waits until it is up. `actor` is who the
    /// log says woke it.
    pub(crate) async fn wake_for_use(&self, id: &str, actor: &str) -> Result<Woken, Refused> {
        let began = Instant::now();
        loop {
            let status = self.read(|s| s.computers.get(id).map(|c| c.status)).ok_or(Refused::NotFound("computer"))?;
            let busy = waking(id).is_some();
            match plan(status) {
                Plan::Ready if !busy => return Ok(Woken::Ready),
                Plan::Waiting => return Ok(Woken::Queued(self.queue_position(id))),
                Plan::Refused(why) => return Err(Refused::Conflict(why.into())),
                Plan::Wake if claim(id) => return self.wake_claimed(id, actor).await.map(|_| self.woken(id)),
                Plan::Ready | Plan::Wake => {}
            }
            if began.elapsed() > WAKE_WAIT {
                return Err(Refused::Failed("the computer did not wake in time".into()));
            }
            tokio::time::sleep(WAKE_POLL).await;
        }
    }

    fn woken(&self, id: &str) -> Woken {
        match self.read(|s| s.computers.get(id).map(|c| c.status)) {
            Some(Status::Queued) => Woken::Queued(self.queue_position(id)),
            _ => Woken::Ready,
        }
    }

    fn queue_position(&self, id: &str) -> usize {
        self.read(|s| s.computers.queue().iter().position(|c| c.id == id).map_or(0, |i| i + 1))
    }

    /// Starts a wake and returns at once, for a person opening the computer.
    pub(crate) fn wake_in_background(self: &Arc<Self>, id: &str, actor: &str) {
        let status = self.read(|s| s.computers.get(id).map(|c| c.status));
        if status.map(plan) != Some(Plan::Wake) || !claim(id) {
            return;
        }
        let (k, id, actor) = (self.clone(), id.to_string(), actor.to_string());
        tokio::spawn(async move {
            if let Err(e) = k.wake_claimed(&id, &actor).await {
                tracing::warn!("computer {id} did not wake: {e}");
            }
        });
    }

    /// The whole wake, by the one caller holding the claim.
    pub(super) async fn wake_claimed(&self, id: &str, actor: &str) -> Result<Value, Refused> {
        let _claim = Claimed(id);
        let socket = self.computer_socket()?;
        let record = |change: Change| async move { self.emit(actor, Ev::Computer(change)).await.map(|_| ()) };
        match boot(&socket, id, || record(Change::Started { id: id.into() })).await.map_err(super::service::failed)? {
            Booted::AtCapacity => {
                record(Change::Queued { id: id.into() }).await.map_err(super::service::failed)?;
                Ok(json!({ "status": "queued", "queue_position": self.queue_position(id) }))
            }
            Booted::Woke(woke) => {
                set_phase(id, Phase::Restoring);
                self.after_boot(&socket, id, &woke).await;
                super::idle::touch(id);
                Ok(json!({ "status": "running", "resumed": woke["resumed"], "replayed": woke["overlay_fresh"] }))
            }
        }
    }
}

/// The guest's desktop, captured and kept as the computer's last screenshot.
pub(super) async fn screenshot(socket: &Path, id: &str) -> Result<String> {
    act_and_capture(socket, id, "computer_screenshot", &json!({})).await
}

const SCREENSHOT_PATH: &str = "/tmp/magnus-shot.png";
const CAPTURE_TIMEOUT_SECONDS: u64 = 60;

/// One desktop action, then a screenshot of what it left.
pub(super) async fn act_and_capture(socket: &Path, id: &str, tool: &str, input: &Value) -> Result<String> {
    let command = crate::boundary::computer_command(tool, input)?;
    let done = client::guest(socket, id, json!({ "op": "exec", "command": command, "timeout_s": CAPTURE_TIMEOUT_SECONDS })).await?;
    if done["exit_code"] != json!(0) {
        return Err(anyhow!("the desktop action failed: {}", done["stderr"].as_str().unwrap_or_default()));
    }
    let shot = client::guest(socket, id, json!({ "op": "read", "path": SCREENSHOT_PATH })).await?;
    let image = shot["data_b64"].as_str().map(String::from).ok_or_else(|| anyhow!("the computer did not return its screen"))?;
    keep_screen(id, &image);
    Ok(image)
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;
