//! Team agent orchestration: delegating connector and personal account requests
//! to a member's own personal agent (`team_ask_personal_agent`).
//!
//! The team agent never uses a member's personal sign-in directly. Instead, it
//! sends a message to the member's personal agent, which runs under that
//! member's own identity, connectors, policies and approvals. When delegated
//! for the requester, the answer returns to the team agent tainted `private`,
//! so the team run's reply stays held for the requester alone until they share
//! it. When delegated to another member, that member's personal agent works for
//! them privately and nothing from their account flows back into the
//! requester's run.
use super::chat_routes::name_of;
use super::{Change, Message, Role, TeamRun, Workspace};
use crate::boundary::Action;
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use crate::state::Status;
use crate::tools::Label;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const HISTORY_MESSAGES: usize = 40;
const MAX_DELEGATE_REQUEST_CHARS: usize = 4000;
const DELEGATE_WAIT_SECS: Duration = Duration::from_secs(5 * 60);
const DELEGATE_POLL_MS: Duration = Duration::from_millis(100);

fn refused(text: String) -> (String, bool, Option<Value>) {
    (text, true, Some(json!({ "untrusted": false, "private": false })))
}

impl Kernel {
    /// What the team orchestrator is told about delegating to personal agents
    /// and which apps the requester has connected on their personal agent
    /// (by app name only, never credentials or content).
    pub(super) fn personal_agent_apps(&self, requester: &str, _w: &Workspace) -> String {
        let title = |c: &Connector| c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name));
        let connected: Vec<String> =
            self.connectors().into_iter().filter(|c| self.connected(c, requester) && !self.lapsed(c, requester)).map(|c| format!("{} ({})", title(&c), c.name)).collect();
        let mut text = String::from(
            "\nPersonal agent orchestration: You are the team orchestrator. Never use a member's personal sign-in yourself; when a request needs a member's personal connected apps (such as their calendar, mail, drive, or code account) or personal context, delegate it to their personal agent.",
        );
        if !connected.is_empty() {
            text.push_str(&format!("\nApps {requester} has connected on their personal agent: {}.", connected.join(", ")));
        }
        text
    }

    /// Sends a request from the team orchestrator to a member's personal agent.
    pub(super) async fn team_ask_personal_agent(self: &Arc<Self>, t: &TeamRun, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let request = a.input["request"].as_str().map(str::trim).unwrap_or_default();
        if request.is_empty() || request.chars().count() > MAX_DELEGATE_REQUEST_CHARS {
            return Ok(refused("a request for the personal agent needs a short, clear instruction".to_string()));
        }
        let target = a.input["member"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&t.requester);
        let Some(role) = self.read(|s| s.team.role(&t.workspace, target)) else {
            return Ok(refused(format!("{target} is not a member of this workspace")));
        };
        if role == Role::Guest {
            return Ok(refused(format!("{target} is a guest in this workspace and cannot have an agent act")));
        }
        if self.person(target).is_none() {
            return Ok(refused(format!("{target} has no account on this server")));
        }

        let app = a.input["app"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let (ws_name, all_messages) = self
            .read(|s| {
                let w = s.team.workspace(&t.workspace)?;
                let msgs = w.channels.get(&t.channel).map(|c| c.messages.clone()).unwrap_or_default();
                Some((w.name.clone(), msgs))
            })
            .unwrap_or_else(|| (t.workspace.clone(), Vec::new()));
        let here: Vec<&Message> = all_messages.iter().filter(|m| t.thread.is_none() || m.thread == t.thread || Some(&m.id) == t.thread.as_ref()).collect();
        let shown = &here[here.len().saturating_sub(HISTORY_MESSAGES)..];
        let history = shown.iter().map(|m| format!("{}: {}", m.sender, m.text)).collect::<Vec<_>>().join("\n");
        let others = shown.iter().any(|m| m.sender != target) || target != t.requester;

        let (parent_untrusted, model) = self.read(|s| s.runs.get(&a.run).map(|r| (r.untrusted, Some((r.config.adapter.clone(), r.config.model.clone()))))).unwrap_or((true, None));
        let label = Label { untrusted: parent_untrusted || others, private: true };
        let app_note = app.map(|name| format!(" (using app `{name}`)")).unwrap_or_default();
        let prompt = format!("The team agent in workspace \"{ws_name}\" delegated this request to you on behalf of {}{app_note}: {request}", t.requester);
        let actor = format!("run:{}", a.run);
        let (task, child_run) = self.create_run(&actor, target, prompt, vec![], NewRun { label: Some(label), model, ..Default::default() }).await?;
        let change =
            Change::Whisper { workspace: t.workspace.clone(), channel: t.channel.clone(), thread: t.thread.clone(), task: task.clone(), owner: target.to_string(), history };
        self.emit(&actor, Ev::Team(change)).await?;
        self.wake(&child_run);

        if target != t.requester {
            return Ok((
                format!(
                    "Sent the request to {target}'s personal agent (task {task}). For privacy, {target}'s personal agent runs under {target}'s own account and replies privately to {target} in this thread until {target} chooses to post it."
                ),
                false,
                Some(json!({ "untrusted": false, "private": false, "task": task, "child_run": child_run, "member": target })),
            ));
        }

        self.wait_for_personal_agent(target, &task, &child_run).await
    }

    async fn wait_for_personal_agent(&self, member: &str, task: &str, child_run: &str) -> Result<(String, bool, Option<Value>)> {
        let started = Instant::now();
        let taint = json!({ "untrusted": true, "private": true, "task": task, "child_run": child_run, "member": member });
        loop {
            let Some(r) = self.read(|s| s.runs.get(child_run).cloned()) else {
                return Err(anyhow!("the personal agent's run is gone"));
            };
            if r.status.terminal() {
                let said = r
                    .summary
                    .clone()
                    .or_else(|| r.replies.last().map(|m| m.text().to_string()))
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "(the personal agent finished without a text reply)".to_string());
                return Ok((format!("Personal agent ({member}): {said}"), r.status == Status::Failed, Some(taint)));
            }
            if r.status == Status::Suspended {
                return Ok((format!("{member}'s personal agent paused in task {task} waiting for {member}'s approval or sign-in in their personal Magnus."), false, Some(taint)));
            }
            if started.elapsed() > DELEGATE_WAIT_SECS {
                return Ok((format!("{member}'s personal agent is still working in task {task}."), false, Some(taint)));
            }
            tokio::time::sleep(DELEGATE_POLL_MS).await;
        }
    }

    /// Active and recent personal agent delegations in the given channels,
    /// formatted for the workspace right side panel. Only the owner sees any
    /// private text; other members see only who is working and the status.
    pub(super) fn panel_delegations(&self, ws: &str, viewer: &str, channels: &[&String]) -> Vec<Value> {
        let items: Vec<(String, String, String, bool, Status, Option<String>)> = self.read(|s| {
            s.team
                .whispers_of(ws)
                .into_iter()
                .filter(|w| channels.contains(&&w.channel))
                .filter_map(|w| {
                    let run = s.tasks.get(&w.task).and_then(|t| s.runs.get(&t.run_id))?;
                    let summary = (w.owner == viewer).then(|| run.summary.clone()).flatten();
                    Some((w.task.clone(), w.channel.clone(), w.owner.clone(), w.shared, run.status, summary))
                })
                .collect()
        });
        items
            .into_iter()
            .rev()
            .take(8)
            .map(|(task, channel, owner, shared, status, summary)| {
                let state = match status {
                    Status::Suspended => "waiting",
                    s if s.terminal() => "done",
                    _ => "running",
                };
                json!({
                    "task": task,
                    "channel": channel,
                    "member": owner,
                    "member_name": name_of(self, &owner),
                    "mine": owner == viewer,
                    "shared": shared,
                    "status": state,
                    "summary": summary,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::admin_app::tests::server_with;
    use crate::boundary::{Action, Issuer};
    use axum::Router;
    use serde_json::json;

    const APPS: &str =
        "[[connectors]]\nname = \"gcal\"\nurl = \"https://gcal.example/mcp\"\ncredential = \"connector_gcal\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n";

    #[tokio::test(flavor = "multi_thread")]
    async fn delegating_to_another_members_personal_agent_starts_their_private_whisper() {
        let s = server_with(APPS, |_| Router::new()).await;
        let sam = s.token("sam").await;
        let _mo = s.token("mo").await;
        let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "ana", "role": "guest" })).await.0, 200);
        let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
        let (task, run_id) = s.k.team_request(&ws, &channel, None, "sam", "Ask Mo's personal agent for Mo's free times".into()).await.unwrap();
        let t = s.k.team_of(&task).unwrap();

        // Delegating to a guest is refused.
        let guest_action = Action {
            run: run_id.clone(),
            call_id: "c1".into(),
            step_id: "s1".into(),
            tool: "team_ask_personal_agent".into(),
            input: json!({ "member": "ana", "request": "Check calendar" }),
            issuer: Issuer::Model,
        };
        let (msg, is_err, _) = s.k.team_ask_personal_agent(&t, &guest_action).await.unwrap();
        assert!(is_err && msg.contains("guest"), "{msg}");

        // Delegating to Mo starts a personal whisper run owned by Mo without tainting Sam's run private.
        let mo_action = Action {
            run: run_id,
            call_id: "c2".into(),
            step_id: "s2".into(),
            tool: "team_ask_personal_agent".into(),
            input: json!({ "member": "mo", "app": "gcal", "request": "List free slots tomorrow afternoon" }),
            issuer: Issuer::Model,
        };
        let (msg, is_err, data) = s.k.team_ask_personal_agent(&t, &mo_action).await.unwrap();
        assert!(!is_err && msg.contains("mo's personal agent"), "{msg}");
        let d = data.unwrap();
        assert_eq!(d["private"], json!(false), "cross-member delegation tainted requester's run with another member's privacy");
        let child_task = d["task"].as_str().unwrap();
        let whisper = s.k.read(|st| st.team.whisper_of(child_task).cloned()).unwrap();
        assert_eq!(whisper.owner, "mo");
        assert_eq!(whisper.channel, channel);
        s.done().await;
    }
}
