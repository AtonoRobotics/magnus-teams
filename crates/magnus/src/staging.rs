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

/// How an out-of-envelope write is staged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageKind {
    /// Real draft in Gmail/Outlook (read back via `Shown`).
    NativeDraft {
        #[serde(default)]
        draft_id: Option<String>,
    },
    /// Calendar event created without sending external invites (`sendUpdates=none`).
    CalendarSilentEvent,
    /// Held inside Magnus for apps without native drafts (Slack, GitHub, Linear, Asana).
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
    pub kind: StageKind,
    /// Server-written sentence from structured fields; never model-written.
    pub sentence: String,
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

/// Classify whether `(app, tool, risk)` can be staged asynchronously, or
/// `None` if it is unstageable (payments, destructive deletes, ACL changes).
pub fn classify_stageable(app: &str, tool: &str, risk: Risk, input: &Value) -> Option<StageKind> {
    if risk == Risk::Financial || !risk.changes_state() {
        return None;
    }
    let lower = tool.to_ascii_lowercase();
    if UNSTAGEABLE_SUBSTRINGS.iter().any(|p| lower.contains(p)) {
        return None;
    }
    if matches!(app, "gmail" | "outlook" | "mail") {
        let draft_id = input["draft_id"].as_str().map(String::from);
        return Some(StageKind::NativeDraft { draft_id });
    }
    if matches!(app, "calendar" | "google_calendar" | "outlook_calendar") {
        return Some(StageKind::CalendarSilentEvent);
    }
    Some(StageKind::MagnusHeld)
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
        };
        self.emit("kernel", Ev::Staged(Change::Created(Box::new(item)))).await?;
        let mode = match meta.kind {
            StageKind::NativeDraft { .. } => "saved as a draft for the person to review and send",
            StageKind::CalendarSilentEvent => "staged without sending external calendar invites (sendUpdates=none)",
            StageKind::MagnusHeld => "staged for the person to review and send asynchronously",
        };
        Ok(format!("Action {id} was {mode} because its destination is outside the current request's envelope ({}). Carry on with the rest of your work.", meta.reason))
    }

    /// Resolve a staged item (sent, discarded, edited, or reconciled after
    /// external Gmail/Outlook draft send/delete).
    pub async fn resolve_staged(&self, id: &str, outcome: Resolution, by: &str, scope: Option<GrantScope>) -> Result<Value> {
        let item = self.read(|s| s.staged.get(id).cloned()).ok_or_else(|| anyhow::anyhow!("unknown staged item {id}"))?;
        let mut grant_id = None;
        if outcome == Resolution::Sent {
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
        Ok(json!({ "id": id, "outcome": outcome, "grant_id": grant_id }))
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
        };
        ledger.apply(&Change::Created(Box::new(item)));
        assert_eq!(ledger.open_for("alice").len(), 1);
        ledger.apply(&Change::Resolved { id: "st1".into(), outcome: Resolution::SentExternally, by: "reconciler".into(), grant_id: None });
        assert!(ledger.open_for("alice").is_empty());
        assert_eq!(ledger.get("st1").and_then(|i| i.resolution), Some(Resolution::SentExternally));
    }
}
