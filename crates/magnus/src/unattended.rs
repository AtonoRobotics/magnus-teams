//! Asks from work nobody is watching (spec rev 140, Outcomes come back to
//! the agent): a routine's ask waits `UNWATCHED_ASK_WAIT_MINUTES`. Then a
//! message stays a draft for its person and is not sent, and anything else
//! keeps its card open: what depends on it waits for the person's answer,
//! and the run carries on with everything that does not.
//!
//! Magnus's own background work (a proactive review, a watch poll) is
//! unwatched work too: its asks wait here the same way (spec rev 142,
//! `background_work`).
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{Item, State, Status};
use crate::tools::Label;
use anyhow::Result;
use chrono::{DateTime, Duration as Span, Utc};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// How long an ask from unwatched work waits for its person.
pub const UNWATCHED_ASK_WAIT_MINUTES: i64 = 30;
const EXPIRY_POLL: Duration = Duration::from_secs(1);
/// What a message's tool is called: a send, a post, a reply, a forward.
const MESSAGE_VERBS: [&str; 5] = ["send", "post", "reply", "forward", "message"];

/// Whether the asked action is a message to someone: it has recipients, or
/// its tool sends, posts or replies. A payment never is.
pub(crate) fn is_message(details: &Value) -> bool {
    if details["risk"] == "financial" {
        return false;
    }
    let recipients = details["destinations"]["recipients"].as_object().is_some_and(|m| m.values().any(|v| v.as_array().is_some_and(|a| !a.is_empty())));
    let tool = details["tool"].as_str().unwrap_or_default();
    let verb = tool.split_once("__").map_or(tool, |(_, r)| r);
    recipients || MESSAGE_VERBS.iter().any(|m| verb.contains(m))
}

/// An open ask of unwatched work whose run still waits on it, past its wait.
fn due(s: &State, item: &Item, now: DateTime<Utc>) -> bool {
    // A sub-agent's ask is as unwatched as the run at its top.
    let top = item.task.as_ref().and_then(|t| s.tasks.get(t)).and_then(|t| s.authority(&t.run_id));
    let unwatched = top.is_some_and(|t| t.schedule.is_some() || t.background());
    let waiting = item.run.as_ref().and_then(|r| s.runs.get(r)).is_some_and(|r| r.status == Status::Suspended && r.waiting_item.as_deref() == Some(item.id.as_str()));
    item.kind == "approval" && item.resolution.is_none() && unwatched && waiting && now - item.created_at >= Span::minutes(UNWATCHED_ASK_WAIT_MINUTES)
}

pub fn schedule(k: Arc<Kernel>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(EXPIRY_POLL);
        loop {
            tick.tick().await;
            if let Err(e) = k.expire_unattended_asks(Utc::now()).await {
                tracing::warn!("expiring unattended asks: {e:#}");
            }
        }
    });
}

impl Kernel {
    pub(crate) async fn expire_unattended_asks(self: &Arc<Self>, now: DateTime<Utc>) -> Result<()> {
        let _decide = self.decide.lock().await;
        let due: Vec<Item> = self.read(|s| s.items.values().filter(|i| due(s, i, now)).cloned().collect());
        for item in due {
            let run = item.run.clone().unwrap_or_default();
            let call_id = item.details["call_id"].as_str().unwrap_or_default().to_string();
            let tool = item.details["tool"].as_str().unwrap_or_default().to_string();
            let mut evs: Vec<(&str, Ev)> = vec![];
            if is_message(&item.details) {
                evs.push(("kernel", Ev::ItemResolved { id: item.id.clone(), decision: "expired".into(), scope: None, text: Some("no one answered in time".into()) }));
                evs.push((
                    "kernel",
                    Ev::Item {
                        id: crate::kernel::new_id("item"),
                        item_kind: "draft".into(),
                        task: item.task.clone(),
                        run: item.run.clone(),
                        person: item.person.clone(),
                        summary: format!("Not sent, kept as a draft: {}", item.summary),
                        details: json!({ "tool": tool, "input": item.details["input"], "destinations": item.details["destinations"], "from_ask": item.id }),
                    },
                ));
                evs.extend(crate::inbox::reply(
                    &run,
                    &call_id,
                    "No one answered in time, so this was not sent. It is kept as a draft for the person. Do not retry it; carry on with what does not depend on it.",
                    true,
                    Label::PRINCIPAL,
                ));
            } else {
                // The card stays open: this waits for the person, the rest goes on.
                evs.extend(crate::inbox::reply(
                    &run,
                    &call_id,
                    "This waits for the person's answer and was not done. Carry on with everything that does not depend on it; you will be told if they allow it.",
                    true,
                    Label::PRINCIPAL,
                ));
            }
            evs.extend(self.resume(&run));
            self.emit_all(evs).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A send stays a draft; a payment or a connector write keeps waiting.
    #[test]
    fn only_a_message_becomes_a_draft() {
        assert!(is_message(&json!({ "tool": "mail__send", "risk": "irreversible" })));
        assert!(is_message(&json!({ "tool": "crm__note", "risk": "reversible_write", "destinations": { "recipients": { "to": ["bob@corp.example"] } } })));
        assert!(!is_message(&json!({ "tool": "crm__update_record", "risk": "reversible_write" })), "a write became a draft");
        assert!(!is_message(&json!({ "tool": "shop__send_payment", "risk": "financial" })), "a payment became a draft");
    }

    #[test]
    fn the_wait_is_thirty_minutes() {
        assert_eq!(UNWATCHED_ASK_WAIT_MINUTES, 30);
    }
}
