//! Draft-first staging for out-of-envelope stageable writes (v2 security architecture).
//!
//! Instead of blocking a run with a modal approval card when a stageable write
//! targets a destination outside the user's envelope (or is a cross-thread
//! reply to an untrusted sender), the kernel stages it as a draft/staged card:
//! - Rendered strictly from service read-back (`Shown`) and server-written
//!   sentences (`asks::sentence`), never a model-written summary (inv6).
//! - Carries destination provenance (where each recipient came from).
//! - Offers fixed kernel-defined scope grants on Send (`once` -> `recipient` -> `domain`).
//! - Tracks full lifecycle via `Ev::Staged(Change::Created | Change::Resolved)`,
//!   including external Gmail/Outlook draft send/delete reconciliation.
use crate::boundary::{Action, Facts, Shown};
use crate::envelope::Destination;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::Run;
use crate::tools::Risk;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod send;
#[cfg(test)]
mod send_tests;

/// What kind of message a staged item is. Every kind is held by Magnus:
/// nothing is created in the app until the person sends it, and then the
/// action itself runs through the gate
/// (decisions/2026-11-23-staged-sends-are-performed.md).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageKind {
    /// A mail send (Gmail, Outlook), with the draft it sends when it names one.
    NativeDraft {
        #[serde(default)]
        draft_id: Option<String>,
    },
    /// A calendar event or invite.
    CalendarSilentEvent,
    /// A chat send or post (Slack, Google Chat).
    MagnusHeld,
}

/// Fixed scope steps offered by the kernel when a person sends a staged item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Once,
    Recipient,
    Domain,
}

/// How a staged item closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Sent,
    Discarded,
    Edited,
    SentExternally,
    DeletedExternally,
}

/// Where a staged action's destination came from, shown on the card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationProvenance {
    pub destination: String,
    pub source: String,
}

/// A staged action awaiting asynchronous human review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StagedItem {
    pub id: String,
    pub run: String,
    pub task: String,
    pub person: String,
    pub tool: String,
    pub app: String,
    #[serde(default)]
    pub app_title: Option<String>,
    /// Written as `stage_kind`: carried in `Ev::Staged`, whose own tag is
    /// `kind`, a field of that name replaced the event's tag, and the event
    /// did not read back (`current_staged` reads those).
    #[serde(rename = "stage_kind")]
    pub kind: StageKind,
    /// Server-written sentence from structured fields; never model-written.
    /// Content, as the input is (`events::CONTENT`): gone once deleted.
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub input: Value,
    /// Service read-back details (`Shown::details`), when available.
    #[serde(default)]
    pub shown: Option<Value>,
    #[serde(default)]
    pub provenance: Vec<DestinationProvenance>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub resolution: Option<Resolution>,
    #[serde(default)]
    pub grant_id: Option<String>,
    /// The approval hash of exactly what was staged (`boundary::shown_hash`):
    /// sending holds only for this, so a draft changed since (one added
    /// Bcc) is not sent. Absent on items staged before it was kept.
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Created(Box<StagedItem>),
    Resolved {
        id: String,
        outcome: Resolution,
        by: String,
        #[serde(default)]
        grant_id: Option<String>,
    },
}

/// A created staged item as written before 2026-11-24: its `kind` field had
/// replaced the event's own tag, so the stored body's `kind` is the stage
/// kind's object. Read as the current form, under `stage_kind`.
pub fn current_staged(body: &Value) -> Option<Value> {
    if !body.get("kind")?.is_object() || body.get("change")?.as_str()? != "created" {
        return None;
    }
    let mut current = body.clone();
    current["stage_kind"] = body["kind"].clone();
    current["kind"] = Value::from("staged");
    Some(current)
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    items: BTreeMap<String, StagedItem>,
}

impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.items.retain(|_, item| item.person != who.person && !who.runs.contains(&item.run));
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Created(item) => {
                self.items.insert(item.id.clone(), (**item).clone());
            }
            Change::Resolved { id, outcome, grant_id, .. } => {
                if let Some(item) = self.items.get_mut(id) {
                    item.resolution = Some(*outcome);
                    if grant_id.is_some() {
                        item.grant_id = grant_id.clone();
                    }
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&StagedItem> {
        self.items.get(id)
    }

    pub fn open_for(&self, person: &str) -> Vec<&StagedItem> {
        self.items.values().filter(|i| i.person == person && i.resolution.is_none()).collect()
    }
}

/// Unstageable tool name substrings: permanent deletes, permission/visibility
/// changes, webhook/OAuth app creation, and VM destination additions always
/// require a modal approval card instead of staging.
const UNSTAGEABLE_SUBSTRINGS: [&str; 9] = ["delete", "remove", "destroy", "purge", "permission", "make_public", "add_member", "webhook", "add_computer_destination"];

/// App-name parts that make an app a calendar, checked before mail so an
/// Outlook calendar is a calendar.
const CALENDAR_APP_PARTS: [&str; 1] = ["calendar"];
/// App-name parts that make an app a mail app.
const MAIL_APP_PARTS: [&str; 3] = ["gmail", "outlook", "mail"];
/// App-name parts that make an app a chat app.
const CHAT_APP_PARTS: [&str; 3] = ["slack", "chat", "discord"];
/// Tool-name parts that make a mail or chat tool a send or a post.
const MESSAGE_SEND_PARTS: [&str; 4] = ["send", "post", "reply", "forward"];
/// Tool-name parts that make a calendar tool an event or invite others see.
const CALENDAR_SEND_PARTS: [&str; 2] = ["event", "invite"];

/// Classify whether `(app, tool, risk)` can be staged asynchronously, or
/// `None` if it cannot. Only a message is stageable: a mail, calendar or
/// chat send or post. Anything else (a sign-in, a secure form, a local
/// command given the network, a payment, a delete, an ACL change) keeps
/// case 3's ask: a held item no one can carry out is no answer to it.
pub fn classify_stageable(app: &str, tool: &str, risk: Risk, input: &Value) -> Option<StageKind> {
    // A task for an outside agent asks: no draft of it can be held there.
    if risk == Risk::Financial || !risk.changes_state() || app == crate::a2a::APP {
        return None;
    }
    let lower = tool.to_ascii_lowercase();
    if UNSTAGEABLE_SUBSTRINGS.iter().any(|p| lower.contains(p)) {
        return None;
    }
    let app = app.to_ascii_lowercase();
    let remote = lower.split_once("__").map_or(lower.as_str(), |(_, r)| r);
    let has = |parts: &[&str], s: &str| parts.iter().any(|p| s.contains(p));
    if has(&CALENDAR_APP_PARTS, &app) {
        return has(&CALENDAR_SEND_PARTS, remote).then_some(StageKind::CalendarSilentEvent);
    }
    if !has(&MESSAGE_SEND_PARTS, remote) {
        return None;
    }
    if has(&MAIL_APP_PARTS, &app) {
        let draft_id = input["draft_id"].as_str().map(String::from);
        return Some(StageKind::NativeDraft { draft_id });
    }
    has(&CHAT_APP_PARTS, &app).then_some(StageKind::MagnusHeld)
}

/// Scope options the kernel offers when a user sends a staged item.
pub fn offered_scopes(dests: &[Destination]) -> Vec<GrantScope> {
    let mut scopes = vec![GrantScope::Once];
    if !dests.is_empty() {
        scopes.push(GrantScope::Recipient);
        if dests.iter().all(|d| matches!(d, Destination::Email { .. })) {
            scopes.push(GrantScope::Domain);
        }
    }
    scopes
}

pub(crate) struct StageMeta<'a> {
    pub kind: StageKind,
    pub dests: &'a [String],
    pub reason: &'a str,
}

impl Kernel {
    /// Stage an out-of-envelope write as a draft/staged card so the run can
    /// continue without blocking on a modal approval.
    pub(crate) async fn stage_action(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>, meta: StageMeta<'_>) -> Result<String> {
        let reason = meta.reason;
        let id = self.stage_item(a, run, f, shown, meta).await?;
        // Nothing exists in the app yet: the model is told it is held, never
        // that a draft was saved (decisions/2026-11-23-staged-sends-are-performed.md).
        Ok(format!(
            "Action {id} was not done: it is held for the person to review and send, because its destination is outside the current request's envelope ({reason}). Nothing went out. Carry on with the rest of your work."
        ))
    }

    /// Record a staged item; its id.
    pub(crate) async fn stage_item(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>, meta: StageMeta<'_>) -> Result<String> {
        let id = new_id("staged");
        let title = f.connector.as_ref().and_then(|c| c.title.clone());
        let sentence = crate::asks::sentence(&a.tool, &f.app, title.as_deref(), f.risk.as_str(), meta.dests);
        let provenance = meta.dests.iter().map(|d| DestinationProvenance { destination: d.clone(), source: meta.reason.to_string() }).collect();
        let item = StagedItem {
            id: id.clone(),
            run: a.run.clone(),
            task: run.task.clone(),
            person: run.person.clone(),
            tool: a.tool.clone(),
            app: f.app.clone(),
            app_title: title,
            kind: meta.kind.clone(),
            sentence,
            input: a.input.clone(),
            shown: shown.map(Shown::details),
            provenance,
            created_at: Utc::now(),
            resolution: None,
            grant_id: None,
            hash: Some(crate::boundary::shown_hash(a, shown)),
        };
        self.emit("kernel", Ev::Staged(Change::Created(Box::new(item)))).await?;
        Ok(id)
    }

    /// Resolve a staged item (sent, discarded, edited, or reconciled after
    /// external Gmail/Outlook draft send/delete). Sent performs the staged
    /// action through the gate (`send::send_staged`), once: an item already
    /// resolved, or being sent, is left as it is.
    pub async fn resolve_staged(self: &std::sync::Arc<Self>, id: &str, outcome: Resolution, by: &str, scope: Option<GrantScope>) -> Result<Value> {
        let Some(_one) = send::Claim::take(id) else { return Ok(json!({ "id": id, "outcome": Value::Null, "sent": false, "why": "it is being resolved already" })) };
        let item = self.read(|s| s.staged.get(id).cloned()).ok_or_else(|| anyhow::anyhow!("unknown staged item {id}"))?;
        if let Some(done) = item.resolution {
            return Ok(json!({ "id": id, "outcome": done, "sent": false, "why": "it was resolved already" }));
        }
        let mut grant_id = None;
        if outcome == Resolution::Sent {
            match self.send_staged(&item, by).await? {
                send::Sent::Done => {}
                send::Sent::Changed(again) => {
                    self.emit("kernel", Ev::Staged(Change::Resolved { id: id.to_string(), outcome: Resolution::Edited, by: by.to_string(), grant_id: None })).await?;
                    return Ok(json!({ "id": id, "outcome": Resolution::Edited, "sent": false, "staged_again": again, "why": "where it goes changed since it was staged" }));
                }
                send::Sent::NotDone(why) => return Ok(json!({ "id": id, "outcome": Value::Null, "sent": false, "why": why })),
            }
            if let Some(sc) = scope.filter(|s| *s != GrantScope::Once) {
                let target_dests: Vec<String> = item
                    .provenance
                    .iter()
                    .filter_map(|p| {
                        let parsed = Destination::parse_for_app(&item.app, &p.destination)?;
                        match sc {
                            GrantScope::Recipient => Some(parsed.display()),
                            GrantScope::Domain => match parsed {
                                Destination::Email { domain, .. } => Some(format!("@{domain}")),
                                _ => None,
                            },
                            GrantScope::Once => None,
                        }
                    })
                    .collect();
                if !target_dests.is_empty() {
                    if let Ok(compiled) = crate::asks::always_rule(&item.person, &item.app, &item.tool, &target_dests) {
                        let rid = new_id("rule");
                        self.emit(
                            by,
                            Ev::Rule(crate::rules::Change::Proposed {
                                id: rid.clone(),
                                person: item.person.clone(),
                                words: Some(format!("Allow {} to {}", item.tool, target_dests.join(", "))),
                                compiled: Some(compiled),
                            }),
                        )
                        .await?;
                        self.emit(by, Ev::Rule(crate::rules::Change::Accepted { id: rid.clone() })).await?;
                        grant_id = Some(rid);
                    }
                }
            }
        }
        self.emit("kernel", Ev::Staged(Change::Resolved { id: id.to_string(), outcome, by: by.to_string(), grant_id: grant_id.clone() })).await?;
        Ok(json!({ "id": id, "outcome": outcome, "grant_id": grant_id, "sent": outcome == Resolution::Sent }))
    }
}

#[derive(Deserialize)]
pub struct ResolveBody {
    pub outcome: Resolution,
    #[serde(default)]
    pub scope: Option<GrantScope>,
}

/// `GET /v1/staged`: open staged drafts/actions for the signed-in person,
/// rendered from server read-back with destination provenance and offered scope steps.
pub async fn list_route(axum::extract::State(k): axum::extract::State<std::sync::Arc<Kernel>>, crate::extract::Caller(p): crate::extract::Caller) -> axum::response::Response {
    use axum::response::IntoResponse;
    let items: Vec<Value> = k.read(|s| {
        s.staged
            .open_for(&p.id)
            .into_iter()
            .map(|item| {
                let dests: Vec<Destination> = item.provenance.iter().filter_map(|pr| Destination::parse_for_app(&item.app, &pr.destination)).collect();
                json!({
                    "id": item.id,
                    "run": item.run,
                    "task": item.task,
                    "tool": item.tool,
                    "app": item.app,
                    "app_title": item.app_title,
                    "kind": item.kind,
                    "sentence": item.sentence,
                    "shown": item.shown,
                    "provenance": item.provenance,
                    "offered_scopes": offered_scopes(&dests),
                    "created_at": item.created_at,
                })
            })
            .collect()
    });
    axum::Json(json!({ "items": items })).into_response()
}

/// `POST /v1/staged/{id}`: send, discard, edit, or reconcile a staged item
/// belonging to the signed-in person, optionally recording a sticky scope grant.
pub async fn resolve_route(
    axum::extract::State(k): axum::extract::State<std::sync::Arc<Kernel>>,
    crate::extract::Caller(p): crate::extract::Caller,
    axum::extract::Path(id): axum::extract::Path<String>,
    crate::extract::Body(body): crate::extract::Body<ResolveBody>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let owner = k.read(|s| s.staged.get(&id).map(|i| (i.person.clone(), i.resolution.is_some())));
    match owner {
        None => return (StatusCode::NOT_FOUND, axum::Json(json!({ "error": "no such staged item" }))).into_response(),
        Some((who, _)) if who != p.id => return (StatusCode::FORBIDDEN, axum::Json(json!({ "error": "not your staged item" }))).into_response(),
        Some((_, true)) => return (StatusCode::CONFLICT, axum::Json(json!({ "error": "staged item already resolved" }))).into_response(),
        Some((_, false)) => {}
    }
    match k.resolve_staged(&id, body.outcome, &format!("person:{}", p.id), body.scope).await {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payments_and_destructive_actions_are_never_stageable() {
        assert!(classify_stageable("payments", "pay", Risk::Financial, &json!({})).is_none());
        assert!(classify_stageable("github", "github__delete_repo", Risk::Irreversible, &json!({})).is_none());
        assert!(classify_stageable("drive", "drive__make_public", Risk::Irreversible, &json!({})).is_none());
        assert!(classify_stageable("computer", "add_computer_destination", Risk::ReversibleWrite, &json!({})).is_none());
    }

    #[test]
    fn email_calendar_and_chat_sends_map_to_expected_stage_kinds() {
        assert_eq!(
            classify_stageable("gmail", "gmail__send_draft", Risk::Irreversible, &json!({ "draft_id": "d1" })),
            Some(StageKind::NativeDraft { draft_id: Some("d1".into()) })
        );
        assert_eq!(classify_stageable("calendar", "calendar__create_event", Risk::Irreversible, &json!({})), Some(StageKind::CalendarSilentEvent));
        assert_eq!(classify_stageable("slack", "slack__post", Risk::Irreversible, &json!({})), Some(StageKind::MagnusHeld));
        assert_eq!(classify_stageable("google-chat", "google-chat__send_message", Risk::Irreversible, &json!({})), Some(StageKind::MagnusHeld));
        assert_eq!(classify_stageable("microsoft-calendar", "microsoft-calendar__respond_to_event", Risk::Irreversible, &json!({})), Some(StageKind::CalendarSilentEvent));
    }

    /// Only a message is staged. A sign-in, a local command given the
    /// network, or an app's write that sends no message keeps case 3's ask:
    /// before, each became a held item that nothing could carry out.
    #[test]
    fn what_is_not_a_message_is_never_stageable() {
        assert!(classify_stageable("browser", "browser_sign_in", Risk::ReversibleWrite, &json!({ "identity": "idn_1" })).is_none());
        assert!(classify_stageable("local", "local_exec", Risk::Irreversible, &json!({ "command": "curl x", "network": true })).is_none());
        assert!(classify_stageable("github", "github__create_issue", Risk::ReversibleWrite, &json!({})).is_none());
        assert!(classify_stageable("google-gmail", "google-gmail__label_message", Risk::ReversibleWrite, &json!({})).is_none());
        assert!(classify_stageable("google-calendar", "google-calendar__set_color", Risk::ReversibleWrite, &json!({})).is_none());
    }

    /// A staged item's event, as the log stores it (`serde_json::to_value`)
    /// and reads it back on a restart.
    #[test]
    fn a_staged_items_event_reads_back_as_written() {
        let item = StagedItem {
            id: "st1".into(),
            run: "r1".into(),
            task: "t1".into(),
            person: "alice".into(),
            tool: "gmail__send_draft".into(),
            app: "gmail".into(),
            app_title: None,
            kind: StageKind::NativeDraft { draft_id: Some("d1".into()) },
            sentence: "s".into(),
            input: json!({ "draft_id": "d1" }),
            shown: None,
            provenance: vec![],
            created_at: Utc::now(),
            resolution: None,
            grant_id: None,
            hash: None,
        };
        let ev = crate::events::Ev::Staged(Change::Created(Box::new(item)));
        let stored = serde_json::to_value(&ev).unwrap();
        let read: crate::events::Ev = serde_json::from_value(stored.clone()).unwrap_or_else(|e| panic!("{e}: {stored}"));
        assert_eq!(serde_json::to_value(&read).unwrap(), stored);
        assert_eq!(stored["kind"], "staged", "the event's tag was replaced: {stored}");
        // As written before: the item's kind in the tag's place.
        let mut old = stored.clone();
        old["kind"] = old["stage_kind"].take();
        old.as_object_mut().unwrap().remove("stage_kind");
        let was = crate::log::parse_event(&old).unwrap().unwrap_or_else(|| panic!("the old form was skipped: {old}"));
        assert_eq!(serde_json::to_value(&was).unwrap(), stored);
    }

    /// What a staged message says and who it goes to is content, and the
    /// event reads once that is deleted.
    #[test]
    fn a_staged_items_words_are_content_and_read_once_deleted() {
        let fields = crate::events::content_fields("staged");
        for f in ["sentence", "input", "shown", "provenance"] {
            assert!(fields.contains(&f), "a staged item's {f} is not content");
        }
        let item = json!({ "kind": "staged", "change": "created", "id": "st1", "run": "r1", "task": "t1", "person": "alice", "tool": "gmail__send_draft", "app": "gmail", "stage_kind": { "kind": "magnus_held" }, "created_at": "2026-11-24T00:00:00Z" });
        assert!(matches!(crate::log::parse_event(&item), Ok(Some(_))), "a staged item with its words deleted does not read");
    }

    #[test]
    fn ledger_closes_staged_items_on_resolution() {
        let mut ledger = Ledger::default();
        let item = StagedItem {
            id: "st1".into(),
            run: "r1".into(),
            task: "t1".into(),
            person: "alice".into(),
            tool: "gmail__send_draft".into(),
            app: "gmail".into(),
            app_title: Some("Gmail".into()),
            kind: StageKind::NativeDraft { draft_id: Some("d1".into()) },
            sentence: "Magnus wants to send draft in Gmail, to bob@vendor.com.".into(),
            input: json!({ "draft_id": "d1" }),
            shown: None,
            provenance: vec![DestinationProvenance { destination: "bob@vendor.com".into(), source: "From untrusted email".into() }],
            created_at: Utc::now(),
            resolution: None,
            grant_id: None,
            hash: None,
        };
        ledger.apply(&Change::Created(Box::new(item)));
        assert_eq!(ledger.open_for("alice").len(), 1);
        ledger.apply(&Change::Resolved { id: "st1".into(), outcome: Resolution::SentExternally, by: "reconciler".into(), grant_id: None });
        assert!(ledger.open_for("alice").is_empty());
        assert_eq!(ledger.get("st1").and_then(|i| i.resolution), Some(Resolution::SentExternally));
    }
}
