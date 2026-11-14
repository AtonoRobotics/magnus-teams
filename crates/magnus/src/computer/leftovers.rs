//! What a takeover leaves running, from the kernel's side
//! (decisions/2026-11-13-takeover-leaves-nothing-running.md).
//!
//! Taking the screen first has the guest record a boundary (sandbox/guest,
//! leftovers.rs). Handing it back has the guest stop everything started
//! since, and the takeover ends only when the guest says it has: otherwise
//! the person keeps the screen, the agent stays paused, and the log and the
//! person are told why. While a handback cleans up, the person's screen is
//! cut and none opens, so nothing is started behind the cleanup.

use super::client;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

/// What the guest names is untrusted: this many names, this long, at most.
const MAX_LOGGED_NAMES: usize = 50;
const MAX_NAME_CHARS: usize = 160;
const MAX_WHY_CHARS: usize = 600;

/// What a clean handback stopped and undid.
#[derive(Debug, PartialEq)]
pub(super) struct Cleared {
    pub stopped: Vec<String>,
    pub stopped_count: u64,
    pub removed: Vec<String>,
}

fn short(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Anything but a plain ok from the guest is unclean.
pub(super) fn cleared(reply: &Value) -> Result<Cleared, String> {
    if reply["ok"] != true {
        return Err(short(reply["error"].as_str().unwrap_or("the computer did not confirm it"), MAX_WHY_CHARS));
    }
    let names = |v: &Value| -> Vec<String> {
        v.as_array().map(|a| a.iter().filter_map(Value::as_str).take(MAX_LOGGED_NAMES).map(|s| short(s, MAX_NAME_CHARS)).collect()).unwrap_or_default()
    };
    let stopped = names(&reply["stopped"]);
    let stopped_count = reply["stopped_count"].as_u64().unwrap_or_default().max(stopped.len() as u64);
    Ok(Cleared { stopped, stopped_count, removed: names(&reply["removed"]) })
}

/// The boundary, recorded before the takeover is.
pub(super) async fn begin(socket: &Path, id: &str) -> Result<()> {
    let reply = client::guest(socket, id, json!({ "op": "takeover_begin" })).await?;
    if reply["ok"] == true {
        return Ok(());
    }
    Err(anyhow!("{}", short(reply["error"].as_str().unwrap_or("the computer did not answer"), MAX_WHY_CHARS)))
}

pub(super) async fn end(socket: &Path, id: &str) -> Result<Cleared, String> {
    match client::guest(socket, id, json!({ "op": "takeover_end" })).await {
        Ok(reply) => cleared(&reply),
        Err(e) => Err(short(&format!("{e:#}"), MAX_WHY_CHARS)),
    }
}

/// The programs a handback would close now. Nothing is stopped.
pub(super) async fn preview(socket: &Path, id: &str) -> Result<Vec<String>> {
    let reply = client::guest(socket, id, json!({ "op": "takeover_leftovers" })).await?;
    if reply["ok"] != true {
        return Err(anyhow!("{}", short(reply["error"].as_str().unwrap_or("the computer did not answer"), MAX_WHY_CHARS)));
    }
    let names = reply["programs"].as_array().map(|a| a.iter().filter_map(Value::as_str).take(MAX_LOGGED_NAMES).map(|s| short(s, MAX_NAME_CHARS)).collect());
    Ok(names.unwrap_or_default())
}

pub(super) fn begin_refused(e: &anyhow::Error) -> String {
    format!(
        "Taking control was refused: the computer could not mark where your session begins ({e:#}), so what you start could not be stopped when you hand back. Try again; if it keeps happening, reset the computer, keeping your files."
    )
}

pub(super) fn unclean(why: &str) -> String {
    format!(
        "The computer could not confirm that nothing started during your takeover is still running ({why}), so the agent stays paused and the computer is still yours. Take control to close it, then choose I'm done again; resetting the computer, keeping your files, also clears it."
    )
}

/// The log's line for a clean handback.
pub(super) fn activity(c: &Cleared) -> String {
    if c.stopped_count == 0 && c.removed.is_empty() {
        return "checked that the takeover left nothing running".into();
    }
    let mut line = format!("stopped what the takeover left running: {}", if c.stopped.is_empty() { "nothing".to_string() } else { c.stopped.join(", ") });
    let more = c.stopped_count.saturating_sub(c.stopped.len() as u64);
    if more > 0 {
        line.push_str(&format!(" and {more} more"));
    }
    if !c.removed.is_empty() {
        line.push_str(&format!("; removed {}", c.removed.join(", ")));
    }
    line
}

/// Handbacks in progress, by computer: two at once each hold it shut.
static HANDING_BACK: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

/// A handback in progress on a computer: its screen opens to no one.
pub(super) struct HandingBack(String);

pub(super) fn handing_back(id: &str) -> HandingBack {
    *HANDING_BACK.lock().unwrap_or_else(|e| e.into_inner()).entry(id.to_string()).or_default() += 1;
    HandingBack(id.to_string())
}

pub(super) fn is_handing_back(id: &str) -> bool {
    HANDING_BACK.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
}

impl Drop for HandingBack {
    fn drop(&mut self) {
        let mut open = HANDING_BACK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = open.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                open.remove(&self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_ok_is_clean_and_what_the_guest_names_is_kept_short() {
        for unclean in [json!({}), json!({ "ok": "true" }), json!({ "error": "2 processes would not stop" }), json!(null)] {
            assert!(cleared(&unclean).is_err(), "{unclean} was taken for clean");
        }
        let long = "x".repeat(10_000);
        let many: Vec<String> = (0..500).map(|i| format!("p{i}")).collect();
        let c = cleared(&json!({ "ok": true, "stopped": many, "stopped_count": 3, "removed": [long] })).unwrap();
        assert_eq!((c.stopped.len(), c.stopped_count), (MAX_LOGGED_NAMES, MAX_LOGGED_NAMES as u64), "the count fell below the names");
        assert_eq!(c.removed[0].len(), MAX_NAME_CHARS);
        assert!(cleared(&json!({ "error": long })).unwrap_err().len() <= MAX_WHY_CHARS);
    }

    #[test]
    fn the_log_names_what_was_stopped() {
        let none = Cleared { stopped: vec![], stopped_count: 0, removed: vec![] };
        assert_eq!(activity(&none), "checked that the takeover left nothing running");
        let some = Cleared { stopped: vec!["recorder (pid 9)".into()], stopped_count: 3, removed: vec!["/etc/xdg/autostart/spy.desktop".into()] };
        assert_eq!(activity(&some), "stopped what the takeover left running: recorder (pid 9) and 2 more; removed /etc/xdg/autostart/spy.desktop");
    }

    #[test]
    fn no_screen_opens_while_a_handback_cleans_up() {
        {
            let _closing = handing_back("hb-1");
            {
                let _twice = handing_back("hb-1");
            }
            assert!(is_handing_back("hb-1"), "one handback ending opened the screen during another");
            assert!(!is_handing_back("hb-2"));
        }
        assert!(!is_handing_back("hb-1"), "a finished handback kept the screen shut");
    }
}
