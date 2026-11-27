//! Direct personal-agent replies in a shared conversation: no Missions, no
//! multi-lane orchestrator, no Living Deliverable.
//!
//! When a member asks `@Magnus` in a shared chat (or a routine / quiet-thread
//! follow-up fires for them), their own personal Magnus runs privately in their
//! boundary. On completion, the reply passes the outbound boundary gate
//! (`lane_release::checked_send`: scrubber, taint, outgoing classifier, Auto
//! Review). If clean, it posts directly to the conversation; if held, it stays
//! private to the asker inline in the chat with a Share / Not now control.

use super::lane_release::{Gate, Send, RELEASE_TOOL};
use super::routes::new_id;
use super::{Change, Message};
use crate::compartment::query_gate::scrub_check;
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use crate::state::Status;
use crate::tools::{Audience, Label};
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

/// Maximum length of a single room prompt passed to the asker's personal agent.
pub(crate) const MAX_REPLY_OBJECTIVE_CHARS: usize = 2_000;
/// Poll interval for the detached reply completion watcher.
const REPLY_WATCH_POLL_MS: Duration = Duration::from_millis(250);
/// Maximum wall-clock time a reply watcher waits before stopping quietly.
const REPLY_WATCH_MAX_SECS: Duration = Duration::from_secs(15 * 60);
/// Audience description passed to Auto Review for a post into a shared chat.
const ROOM_AUDIENCE: &str = "it is posted into a shared conversation, where its other members, who are other people, read it";

/// Why a reply that would have posted itself was held privately for its asker.
static HELD_REASONS: LazyLock<RwLock<HashMap<String, String>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) fn set_held_reason(task: &str, why: String) {
    if let Ok(mut map) = HELD_REASONS.write() {
        map.insert(task.to_string(), why);
    }
}

pub(super) fn held_reason(task: &str) -> Option<String> {
    HELD_REASONS.read().ok().and_then(|map| map.get(task).cloned())
}

pub(super) fn clear_held_reason(task: &str) {
    if let Ok(mut map) = HELD_REASONS.write() {
        map.remove(task);
    }
}

impl Kernel {
    /// Runs `person`'s own personal Magnus for a shared conversation ask.
    /// When `hold` is false, a clean reply that passes the outbound gate posts
    /// directly to the conversation; otherwise it stays private to `person`
    /// inline in the chat until they choose to share it.
    #[allow(clippy::too_many_arguments)]
    pub async fn reply_in_room(
        self: &Arc<Self>,
        workspace: &str,
        channel: &str,
        person: &str,
        objective: &str,
        thread: Option<String>,
        pick: crate::say::Pick,
        hold: bool,
    ) -> Result<(String, String)> {
        self.reply_in_room_as(&format!("person:{person}"), workspace, channel, person, objective, thread, None, pick, hold, None).await
    }

    /// Runs `person`'s own personal Magnus in a shared conversation on behalf
    /// of `actor` (`person:<id>` or `"poll"` for routines and quiet-thread
    /// follow-ups). When `routine` is `Some((id, at))`, `RoutineRan` is
    /// emitted before the run wakes so `State::background_asks` knows it is a
    /// routine run that asks no one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn reply_in_room_as(
        self: &Arc<Self>,
        actor: &str,
        workspace: &str,
        channel: &str,
        person: &str,
        objective: &str,
        thread: Option<String>,
        quarantine: Option<serde_json::Value>,
        pick: crate::say::Pick,
        hold: bool,
        routine: Option<(String, chrono::DateTime<chrono::Utc>)>,
    ) -> Result<(String, String)> {
        let objective = objective.trim();
        if objective.is_empty() || objective.chars().count() > MAX_REPLY_OBJECTIVE_CHARS {
            anyhow::bail!("prompt must be between 1 and {MAX_REPLY_OBJECTIVE_CHARS} characters");
        }
        if let Some(reason) = scrub_check(objective) {
            anyhow::bail!("refused prompt: {reason}");
        }
        if !super::routes::has_channel(self, workspace, channel, person) || self.person(person).is_none() {
            anyhow::bail!("{person} is not a member in this conversation of workspace {workspace}");
        }
        if let Some(e) = self.refuse_new(person, false) {
            return Err(e);
        }
        self.admit_run(person, pick.model.clone(), self.cfg.budget.clone()).await?;

        let ws_name = self.read(|s| s.team.workspace(workspace).map(|w| w.name.clone())).unwrap_or_else(|| workspace.to_string());
        let prompt = format!(
            "Conversation \"{channel}\" in shared project \"{ws_name}\" asked your personal Magnus:\n\n{objective}\n\nReturn a concise, scrubbed reply containing only what was requested—never raw credentials, private emails, or unrelated personal records."
        );
        let others_base = self.read(|s| super::agent::lane_base_from_others(s, workspace, channel, person, person));
        let label = Label { untrusted: others_base, private: true };
        let (task, run) =
            self.create_run(actor, person, prompt, vec![], NewRun { label: Some(label), model: pick.model, reasoning: pick.reasoning, quarantine, ..Default::default() }).await?;

        self.emit(
            actor,
            Ev::Team(Change::Whisper {
                workspace: workspace.to_string(),
                channel: channel.to_string(),
                thread: thread.clone(),
                task: task.clone(),
                owner: person.to_string(),
                history: String::new(),
            }),
        )
        .await?;

        if let Some((routine_id, at)) = routine {
            self.emit(actor, Ev::Team(Change::RoutineRan { workspace: workspace.to_string(), id: routine_id, task: task.clone(), at })).await?;
        }

        self.wake(&run);
        if !hold {
            self.spawn_reply_watcher(workspace.to_string(), channel.to_string(), person.to_string(), task.clone(), run.clone(), thread);
        }
        Ok((task, run))
    }

    fn spawn_reply_watcher(self: &Arc<Self>, workspace: String, channel: String, person: String, task: String, run_id: String, thread: Option<String>) {
        let k = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                let Some(r) = k.read(|s| s.runs.get(&run_id).cloned()) else {
                    return;
                };
                if r.status.terminal() {
                    if r.status != Status::Completed {
                        return;
                    }
                    let summary = reply_summary(&r);
                    let gate = k.reply_release_checks(&workspace, &channel, &person, &task, &r, &summary).await;
                    match gate {
                        Gate::Post => {
                            if !super::routes::has_channel(&k, &workspace, &channel, &person) {
                                return;
                            }
                            let message = Message { id: new_id(), sender: format!("agent:{person}"), text: summary, thread, at: chrono::Utc::now() };
                            let actor = format!("person:{person}");
                            if k.emit(&actor, Ev::Team(Change::Message { workspace: workspace.clone(), channel, message })).await.is_ok() {
                                let _ = k.emit(&actor, Ev::Team(Change::WhisperShared { workspace, task: task.clone() })).await;
                                clear_held_reason(&task);
                            }
                        }
                        Gate::Hold { why, .. } => {
                            tracing::info!("shared chat reply {task} held for {person}: {why}");
                            set_held_reason(&task, why);
                        }
                    }
                    return;
                }
                if started.elapsed() > REPLY_WATCH_MAX_SECS {
                    return;
                }
                tokio::time::sleep(REPLY_WATCH_POLL_MS).await;
            }
        });
    }

    async fn reply_release_checks(&self, workspace: &str, channel: &str, person: &str, task: &str, run: &crate::state::Run, summary: &str) -> Gate {
        let place = self.read(|s| {
            let w = s.team.workspace(workspace)?;
            let room = w.channels.get(channel)?;
            let readers: Vec<String> = w.members.keys().filter(|p| p.as_str() != person && w.sees(channel, p)).cloned().collect();
            Some((w.name.clone(), room.name.clone(), readers))
        });
        let Some((ws_name, room_name, readers)) = place else {
            return Gate::Hold { why: "its conversation could not be found".into(), scrub_flagged: false };
        };
        let send = Send {
            run,
            call_id: format!("reply:{task}"),
            tool: RELEASE_TOOL,
            app: "team",
            audience: Audience::Internal,
            input: json!({ "workspace": ws_name, "room": room_name, "text": summary }),
            readers: ("room_members", readers),
            text: summary,
            content: vec![],
            to: "the conversation",
            covered: false,
            standing: format!("the person asked for this in the conversation themselves; {ROOM_AUDIENCE}"),
        };
        self.checked_send(send).await
    }
}

fn reply_summary(r: &crate::state::Run) -> String {
    let deliverable = r.outcome_view.as_ref().map(crate::sub_agents::render_deliverable).unwrap_or_default();
    let raw_summary = r.summary.clone().or_else(|| r.replies.last().map(|m| m.text().to_string())).unwrap_or_default();
    if !deliverable.is_empty() {
        if !raw_summary.trim().is_empty() && !deliverable.contains(raw_summary.trim()) {
            format!("{deliverable}\n\nSummary: {}", raw_summary.trim())
        } else {
            deliverable
        }
    } else if !raw_summary.trim().is_empty() {
        raw_summary
    } else {
        "(finished without a text summary)".to_string()
    }
}
