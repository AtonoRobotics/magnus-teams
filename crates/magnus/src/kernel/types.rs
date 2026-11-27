//! Supporting types for `Kernel` (`Capture`, `KillSwitchOn`, `Admitted`, `NewRun`, `Live`).

use crate::events::Ev;
use std::collections::HashMap;
use std::sync::Mutex;

/// Capture overhead: the cost of writing each step to the log (V6).
#[derive(Default, Clone, serde::Serialize)]
pub struct Capture {
    pub appends: u64,
    pub total_us: u64,
    pub max_us: u64,
    /// The most recent append times, for percentiles.
    #[serde(skip)]
    pub recent: std::collections::VecDeque<u64>,
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// New work refused because the kill switch is on.
#[derive(Debug)]
pub struct KillSwitchOn;

impl std::fmt::Display for KillSwitchOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The kill switch is on: nothing new starts until the owner releases it.")
    }
}

impl std::error::Error for KillSwitchOn {}

/// A run `admit_new` let start: its ids, and the events that record it
/// (its `TaskCreated`, and its team thread with it), each as `actor`.
pub(crate) struct Admitted {
    pub task: String,
    pub run: String,
    pub actor: String,
    pub evs: Vec<Ev>,
}

/// How a new run differs from a plain task.
#[derive(Default)]
pub struct NewRun {
    pub experiment: Option<crate::events::ExperimentTag>,
    pub chat: Option<crate::events::ChatOrigin>,
    pub label: Option<crate::tools::Label>,
    pub budget: Option<crate::config::Budget>,
    /// Fixed ids, so a resent request finds the task it already created.
    pub ids: Option<(String, String)>,
    pub model: Option<(String, String)>,
    pub reasoning: Option<String>,
    pub title: Option<String>,
    pub files: Vec<String>,
    pub project: Option<String>,
    /// Removable modules switched on or off for this run alone.
    pub modules: std::collections::BTreeMap<String, bool>,
    pub scope: Option<String>,
    pub grant: Option<String>,
    /// The team thread it serves, recorded with the task.
    pub team: Option<crate::team::TeamRun>,
    /// Its untrusted reads are quarantined, under this schema (`quarantine_runtime`).
    pub quarantine: Option<serde_json::Value>,
}

pub(crate) fn notice_event(task: Option<String>, run: Option<String>, person: String, summary: String, details: serde_json::Value) -> (String, Ev) {
    let id = new_id("item");
    (id.clone(), Ev::Item { id, item_kind: "notice".into(), task, run, person, summary, details })
}

/// Model text as it streams, before the reply is appended to the log (K6),
/// and each run's reply so far, so a view that opens mid-reply starts with
/// what was already said.
pub struct Live {
    pub(super) tx: tokio::sync::broadcast::Sender<(String, String)>,
    pub(super) so_far: Mutex<HashMap<String, String>>,
}

impl Live {
    pub fn send(&self, run: &str, text: &str) {
        let mut so_far = self.so_far.lock().unwrap_or_else(|e| e.into_inner());
        so_far.entry(run.to_string()).or_default().push_str(text);
        let _ = self.tx.send((run.to_string(), text.to_string()));
    }

    /// The run's reply is complete (or failed): nothing more streams for it.
    pub fn end(&self, run: &str) {
        self.so_far.lock().unwrap_or_else(|e| e.into_inner()).remove(run);
    }

    /// The run's reply so far and what streams after it, with nothing lost
    /// or repeated between the two.
    pub fn subscribe(&self, run: &str) -> (String, tokio::sync::broadcast::Receiver<(String, String)>) {
        let so_far = self.so_far.lock().unwrap_or_else(|e| e.into_inner());
        (so_far.get(run).cloned().unwrap_or_default(), self.tx.subscribe())
    }
}
