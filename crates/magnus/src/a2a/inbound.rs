//! An outside agent asking Magnus: the one JSON-RPC endpoint (A2A 1.0,
//! JSON-RPC binding: SendMessage, GetTask, CancelTask). Every request
//! authenticates with the key the card's scheme declares, is checked against
//! its Cedar principal, and only then may create anything; whatever is not
//! admitted is refused with nothing run. A request names a person, where it
//! lands in their inbox with guest standing, or a shared project it is a
//! guest of, where it is posted for the members to read. A task id is the
//! sending principal's own, made from its message id, so a replay finds the
//! task it already created. Each agent may send a named number an hour.

use super::connect::inbound_key_name;
use super::ledger::{Change, Inbound, OutsideAgent, Target};
use super::{CANCELED, MAX_REQUESTS_PER_AGENT_PER_HOUR, MAX_TEXT_CHARS, SUBMITTED, WORKING};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::State;
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;

/// JSON-RPC 2.0's own errors.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
/// A2A 1.0's errors.
const TASK_NOT_FOUND: i64 = -32001;
const TASK_NOT_CANCELABLE: i64 = -32002;
const UNSUPPORTED_OPERATION: i64 = -32004;
const VERSION_NOT_SUPPORTED: i64 = -32009;
/// This server's own, in JSON-RPC's range for them.
pub(crate) const NOT_AUTHENTICATED: i64 = -32050;
pub(crate) const NOT_ADMITTED: i64 = -32051;
pub(crate) const TOO_MANY_REQUESTS: i64 = -32052;

/// The inbox card a request to a person makes.
pub const REQUEST_ITEM: &str = "outside_request";
/// The window requests are counted over, in seconds.
const RATE_WINDOW_SECS: i64 = 3_600;
/// The channel a request to a shared project is posted in, where it has one.
const PROJECT_CHANNEL: &str = "general";

/// Where a request landed: its inbox card, or its project message
/// (channel, message), and the events that put it there.
type Landed = (Option<String>, Option<(String, String)>, Vec<Ev>);

/// A JSON-RPC answer, and the HTTP status it goes with.
pub(crate) type Answer = (StatusCode, Value);

fn error(st: StatusCode, id: &Value, code: i64, message: impl Into<String>) -> Answer {
    (st, json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } }))
}

fn result(id: &Value, r: Value) -> Answer {
    (StatusCode::OK, json!({ "jsonrpc": "2.0", "id": id, "result": r }))
}

fn digest(s: &str) -> ring::digest::Digest {
    ring::digest::digest(&ring::digest::SHA256, s.as_bytes())
}

/// The task id a request gets: its sender's own, from its message id, so
/// the same message from the same agent is always the same task.
pub(crate) fn task_id(agent: &str, message_id: &str) -> String {
    let d = digest(&format!("{agent}\n{message_id}"));
    let hex: String = d.as_ref().iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!("in-{hex}")
}

/// Who a request names, from its metadata (`{"to": {"person": ..}}` or
/// `{"to": {"project": ..}}`), on the message or on the request.
fn target(params: &Value) -> Option<Target> {
    let to = params["message"]["metadata"].get("to").or_else(|| params["metadata"].get("to"))?;
    serde_json::from_value(to.clone()).ok()
}

/// A request as A2A's Task: its state and what has been answered so far.
/// To a person, the reply they released; to a shared project, what its
/// members posted in reply to it.
pub(crate) fn task_view(s: &State, i: &Inbound) -> Value {
    let mut artifacts: Vec<Value> = vec![];
    if let Some(reply) = &i.reply {
        artifacts.push(json!({ "artifactId": "reply", "name": "Reply", "parts": [{ "text": reply }] }));
    }
    if let (Target::Project(w), Some((channel, msg))) = (&i.to, &i.message) {
        let replies = s
            .team
            .workspace(w)
            .and_then(|w| w.channels.get(channel))
            .into_iter()
            .flat_map(|c| c.messages.iter())
            .filter(|m| m.thread.as_deref() == Some(msg) && !m.sender.starts_with(SENDER_PREFIX));
        artifacts.extend(replies.map(|m| json!({ "artifactId": m.id, "parts": [{ "text": m.text }] })));
    }
    let state = if i.state == SUBMITTED && !artifacts.is_empty() { WORKING } else { i.state.as_str() };
    json!({ "id": i.task, "contextId": i.task, "status": { "state": state, "timestamp": i.at.to_rfc3339() }, "artifacts": artifacts })
}

/// How an outside agent's posts are signed in a shared project: never a
/// member, so no agent answers for it and its words are someone else's.
pub(crate) const SENDER_PREFIX: &str = "outside_agent:";

impl Kernel {
    /// The connected agent whose key this is, by the key's own id and the
    /// vault's copy of it.
    fn authenticated(&self, key: Option<&str>) -> Option<OutsideAgent> {
        let key = key?.trim();
        let (id, _) = key.split_once('.')?;
        let agent = self.read(|s| s.outside_agents.agent(id).cloned())?;
        let kept = self.vault.get(&inbound_key_name(id)).ok()??;
        (digest(kept.expose()).as_ref() == digest(key).as_ref()).then_some(agent)
    }

    /// One JSON-RPC request to the endpoint, with the key and protocol
    /// version it came with.
    pub(crate) async fn outside_request(self: &Arc<Self>, key: Option<&str>, version: Option<&str>, body: &[u8]) -> Answer {
        let Ok(req) = serde_json::from_slice::<Value>(body) else {
            return error(StatusCode::BAD_REQUEST, &Value::Null, PARSE_ERROR, "the request is not JSON");
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let Some(agent) = self.authenticated(key) else {
            return error(StatusCode::UNAUTHORIZED, &id, NOT_AUTHENTICATED, format!("send the key you were given in the {} header", super::KEY_HEADER));
        };
        if !version.is_some_and(|v| v.trim() == super::PROTOCOL_VERSION || v.trim().starts_with("1.")) {
            return error(StatusCode::BAD_REQUEST, &id, VERSION_NOT_SUPPORTED, "this server speaks A2A 1.0: send A2A-Version: 1.0");
        }
        if req["jsonrpc"] != "2.0" {
            return error(StatusCode::BAD_REQUEST, &id, INVALID_REQUEST, "not a JSON-RPC 2.0 request");
        }
        let params = &req["params"];
        match req["method"].as_str().unwrap_or_default() {
            "SendMessage" => self.received(&agent, &id, params).await,
            "GetTask" => match self.read(|s| s.outside_agents.inbound_of(&agent.id, params["id"].as_str().unwrap_or_default()).map(|i| task_view(s, i))) {
                Some(t) => result(&id, t),
                None => error(StatusCode::OK, &id, TASK_NOT_FOUND, "no such task of yours"),
            },
            "CancelTask" => self.cancelled_by_agent(&agent, &id, params["id"].as_str().unwrap_or_default()).await,
            "SendStreamingMessage"
            | "SubscribeToTask"
            | "ListTasks"
            | "CreateTaskPushNotificationConfig"
            | "GetTaskPushNotificationConfig"
            | "ListTaskPushNotificationConfigs"
            | "DeleteTaskPushNotificationConfig"
            | "GetExtendedAgentCard" => error(StatusCode::OK, &id, UNSUPPORTED_OPERATION, "this server answers SendMessage, GetTask and CancelTask"),
            m => error(StatusCode::OK, &id, METHOD_NOT_FOUND, format!("no method {m}")),
        }
    }

    /// A new request: admitted, then created once, then answered as its task.
    async fn received(self: &Arc<Self>, agent: &OutsideAgent, id: &Value, params: &Value) -> Answer {
        let msg = &params["message"];
        let Some(message_id) = msg["messageId"].as_str().map(str::trim).filter(|m| !m.is_empty()) else {
            return error(StatusCode::OK, id, INVALID_PARAMS, "the message has no messageId");
        };
        if msg.get("taskId").is_some_and(|t| !t.is_null()) {
            return error(StatusCode::OK, id, UNSUPPORTED_OPERATION, "each request is a task of its own: send it without a taskId");
        }
        let text = super::parts_text(&msg["parts"]);
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return error(StatusCode::OK, id, INVALID_PARAMS, format!("the request must be 1 to {MAX_TEXT_CHARS} characters of text"));
        }
        let Some(to) = target(params) else {
            return error(StatusCode::OK, id, INVALID_PARAMS, "name whom you ask in the message's metadata: {\"to\": {\"person\": \"<id>\"}} or {\"to\": {\"project\": \"<id>\"}}");
        };
        // Its Cedar principal first: anything no policy admits is refused here.
        let policies: Vec<String> = self.read(|s| s.outside_agents.admissions(&agent.id).iter().map(|(_, p)| p.clone()).collect());
        match crate::policy::outside::admits(&policies, &agent.id, &to) {
            Ok(true) => {}
            Ok(false) => return error(StatusCode::FORBIDDEN, id, NOT_ADMITTED, "you are not admitted to ask that person or shared project"),
            Err(e) => {
                tracing::warn!("an outside agent's policy: {e:#}");
                return error(StatusCode::INTERNAL_SERVER_ERROR, id, INTERNAL_ERROR, "the request could not be checked");
            }
        }
        let task = task_id(&agent.id, message_id);
        let _decide = self.decide.lock().await;
        // A replay finds the task it made.
        if let Some(t) = self.read(|s| s.outside_agents.inbound_of(&agent.id, &task).map(|i| task_view(s, i))) {
            return result(id, json!({ "task": t }));
        }
        let since = chrono::Utc::now() - chrono::Duration::seconds(RATE_WINDOW_SECS);
        if self.read(|s| s.outside_agents.received_since(&agent.id, since)) >= MAX_REQUESTS_PER_AGENT_PER_HOUR {
            return error(StatusCode::TOO_MANY_REQUESTS, id, TOO_MANY_REQUESTS, format!("at most {MAX_REQUESTS_PER_AGENT_PER_HOUR} requests an hour"));
        }
        let landed = match &to {
            Target::Person(p) => self.request_item(agent, &task, p, text),
            Target::Project(w) => match self.request_post(agent, w, text) {
                Some(evs) => evs,
                None => return error(StatusCode::OK, id, INVALID_PARAMS, "that shared project has no channel to post in"),
            },
        };
        let (item, message, mut evs) = landed;
        evs.push(Ev::OutsideAgent(Change::RequestReceived { agent: agent.id.clone(), task: task.clone(), message_id: message_id.into(), to, text: text.into(), item, message }));
        let actor = format!("{SENDER_PREFIX}{}", agent.id);
        if let Err(e) = self.emit_all(evs.into_iter().map(|e| (actor.as_str(), e)).collect()).await {
            tracing::warn!("an outside agent's request: {e:#}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, id, INTERNAL_ERROR, "the request could not be recorded");
        }
        match self.read(|s| s.outside_agents.inbound_of(&agent.id, &task).map(|i| task_view(s, i))) {
            Some(t) => result(id, json!({ "task": t })),
            None => error(StatusCode::INTERNAL_SERVER_ERROR, id, INTERNAL_ERROR, "the request could not be recorded"),
        }
    }

    /// A request to a person: a card in their inbox with guest standing.
    fn request_item(&self, agent: &OutsideAgent, task: &str, person: &str, text: &str) -> Landed {
        let item = new_id("item");
        let first: String = text.lines().next().unwrap_or_default().chars().take(140).collect();
        let ev = Ev::Item {
            id: item.clone(),
            item_kind: REQUEST_ITEM.into(),
            task: None,
            run: None,
            person: person.into(),
            summary: format!("{} asks: {first}", agent.name),
            details: json!({ "agent": agent.id, "agent_name": agent.name, "request": task, "text": text, "standing": "guest", "untrusted": true }),
        };
        (Some(item), None, vec![ev])
    }

    /// A request to a shared project the agent is a guest of: a message its
    /// members read, signed as the agent, which no agent answers for it.
    fn request_post(&self, agent: &OutsideAgent, workspace: &str, text: &str) -> Option<Landed> {
        let channel = self.read(|s| {
            let w = s.team.workspace(workspace)?;
            w.channels.iter().find(|(_, c)| c.name == PROJECT_CHANNEL).or_else(|| w.channels.iter().next()).map(|(id, _)| id.clone())
        })?;
        let msg = new_id("msg");
        let message = crate::team::Message {
            id: msg.clone(),
            sender: format!("{SENDER_PREFIX}{}", agent.id),
            text: format!("{} (outside agent, guest) asks:\n{text}", agent.name),
            thread: None,
            at: chrono::Utc::now(),
        };
        let ev = Ev::Team(crate::team::Change::Message { workspace: workspace.into(), channel: channel.clone(), message });
        Some((None, Some((channel, msg)), vec![ev]))
    }

    async fn cancelled_by_agent(self: &Arc<Self>, agent: &OutsideAgent, id: &Value, task: &str) -> Answer {
        let _decide = self.decide.lock().await;
        let Some(i) = self.read(|s| s.outside_agents.inbound_of(&agent.id, task).cloned()) else {
            return error(StatusCode::OK, id, TASK_NOT_FOUND, "no such task of yours");
        };
        if super::terminal(&i.state) {
            return error(StatusCode::OK, id, TASK_NOT_CANCELABLE, format!("the task has ended ({})", i.state));
        }
        if let Err(e) = self.end_request(&i, CANCELED, "the outside agent cancelled it").await {
            tracing::warn!("cancelling an outside request: {e:#}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, id, INTERNAL_ERROR, "the task could not be cancelled");
        }
        match self.read(|s| s.outside_agents.inbound(task).map(|i| task_view(s, i))) {
            Some(t) => result(id, t),
            None => error(StatusCode::OK, id, TASK_NOT_FOUND, "no such task of yours"),
        }
    }

    /// End a request: its open cards close, and a run taken up for it stops.
    /// One write.
    pub(crate) async fn end_request(&self, i: &Inbound, state: &str, reason: &str) -> anyhow::Result<()> {
        let mut evs: Vec<(&str, Ev)> = vec![("kernel", Ev::OutsideAgent(Change::RequestEnded { task: i.task.clone(), state: state.into(), reason: reason.into() }))];
        let open: Vec<String> = self.read(|s| i.item.iter().chain(i.held.iter()).filter(|c| s.inbox.items().get(*c).is_some_and(|c| c.resolution.is_none())).cloned().collect());
        for c in open {
            evs.push(("kernel", Ev::ItemResolved { id: c, decision: "withdrawn".into(), scope: None, text: Some(reason.into()) }));
        }
        let going = i.run.clone().filter(|r| self.read(|s| s.runs.get(r).is_some_and(|r| !r.status.terminal())));
        match going {
            Some(run) => {
                evs.push(("kernel", Ev::Cancelled { run: run.clone(), source: None, turn: None }));
                self.cancel_tree(&run, evs).await
            }
            None => self.emit_all(evs).await.map(|_| ()),
        }
    }

    /// End every open request `agent` sent; the number ended.
    pub(crate) async fn end_requests(&self, agent: &str, reason: &str) -> anyhow::Result<usize> {
        let _decide = self.decide.lock().await;
        let open = self.read(|s| s.outside_agents.open_inbound(agent));
        for i in &open {
            self.end_request(i, CANCELED, reason).await?;
        }
        Ok(open.len())
    }
}
