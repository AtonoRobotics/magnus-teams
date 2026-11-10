//! Who is here and who is typing. These are moments, not history, so they
//! live in memory only and are never written to the log.
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Someone seen this recently is shown as here.
const ONLINE_WINDOW_SECS: u64 = 90;
/// A typing signal shows this long unless renewed.
const TYPING_SHOWN_SECS: u64 = 6;
const ONLINE_WINDOW: Duration = Duration::from_secs(ONLINE_WINDOW_SECS);
const TYPING_SHOWN: Duration = Duration::from_secs(TYPING_SHOWN_SECS);

#[derive(Default)]
struct Live {
    /// Workspace, then person.
    seen: HashMap<String, HashMap<String, Instant>>,
    /// Workspace and channel, then person.
    typing: HashMap<(String, String), HashMap<String, Instant>>,
}

static LIVE: LazyLock<Mutex<Live>> = LazyLock::new(Mutex::default);

fn live() -> MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn seen(workspace: &str, person: &str) {
    live().seen.entry(workspace.to_string()).or_default().insert(person.to_string(), Instant::now());
}

pub fn online(workspace: &str) -> Vec<String> {
    let mut l = live();
    let Some(people) = l.seen.get_mut(workspace) else { return vec![] };
    people.retain(|_, at| at.elapsed() < ONLINE_WINDOW);
    let mut out: Vec<String> = people.keys().cloned().collect();
    out.sort();
    out
}

pub fn typing(workspace: &str, channel: &str, person: &str) {
    live().typing.entry((workspace.to_string(), channel.to_string())).or_default().insert(person.to_string(), Instant::now());
}

/// Stops showing someone as typing: they sent what they wrote.
pub fn sent(workspace: &str, channel: &str, person: &str) {
    if let Some(people) = live().typing.get_mut(&(workspace.to_string(), channel.to_string())) {
        people.remove(person);
    }
}

pub fn typing_in(workspace: &str, channel: &str) -> Vec<String> {
    let mut l = live();
    let Some(people) = l.typing.get_mut(&(workspace.to_string(), channel.to_string())) else { return vec![] };
    people.retain(|_, at| at.elapsed() < TYPING_SHOWN);
    let mut out: Vec<String> = people.keys().cloned().collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_shows_until_the_message_is_sent() {
        let ws = uuid::Uuid::new_v4().to_string();
        typing(&ws, "c", "bob");
        assert_eq!(typing_in(&ws, "c"), vec!["bob"]);
        assert!(typing_in(&ws, "other").is_empty());
        sent(&ws, "c", "bob");
        assert!(typing_in(&ws, "c").is_empty());
    }

    #[test]
    fn someone_seen_is_online_in_that_workspace_only() {
        let ws = uuid::Uuid::new_v4().to_string();
        seen(&ws, "ada");
        assert_eq!(online(&ws), vec!["ada"]);
        assert!(online("elsewhere-never-used").is_empty());
    }
}
