//! Asks nobody was there to answer. An ask raised by a routine expires after
//! the configured time and nothing runs: a message becomes a draft for its
//! person, and the run is told it was not sent. An install or a payment
//! keeps its ask, and its task keeps waiting.
//!
//! Magnus's own background work (a proactive review, a watch poll) raises no
//! asks at all (owner, 2026-11-07), so none reaches this.
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Item;
use crate::tools::Label;
use anyhow::Result;
use chrono::{DateTime, Duration as Span, Utc};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const EXPIRY_POLL: Duration = Duration::from_secs(1);

/// An install or a payment waits with its ask rather than becoming a draft.
fn waits(item: &Item) -> bool {
    item.details["risk"] == "financial" || item.details["tool"].as_str().is_some_and(|t| t.contains("install"))
}

fn expired(item: &Item, unattended: bool, now: DateTime<Utc>, expiry: Span) -> bool {
    item.kind == "approval" && item.resolution.is_none() && unattended && !waits(item) && now - item.created_at >= expiry
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
        let expiry = Span::seconds(i64::try_from(self.cfg.auto_review.unattended_ask_expiry_seconds)?);
        let due: Vec<Item> = self.read(|s| {
            s.items
                .values()
                .filter(|i| {
                    // A sub-agent's ask is as unattended as the run at its top.
                    let top = i.task.as_ref().and_then(|t| s.tasks.get(t)).and_then(|t| s.authority(&t.run_id));
                    let unattended = top.is_some_and(|t| t.schedule.is_some() || t.background());
                    expired(i, unattended, now, expiry)
                })
                .cloned()
                .collect()
        });
        for item in due {
            let run = item.run.clone().unwrap_or_default();
            let call_id = item.details["call_id"].as_str().unwrap_or_default().to_string();
            let tool = item.details["tool"].as_str().unwrap_or_default().to_string();
            let mut evs: Vec<(&str, Ev)> = vec![
                ("kernel", Ev::ItemResolved { id: item.id.clone(), decision: "expired".into(), scope: None, text: Some("no one answered in time".into()) }),
                (
                    "kernel",
                    Ev::Item {
                        id: crate::kernel::new_id("item"),
                        item_kind: "draft".into(),
                        task: item.task.clone(),
                        run: item.run.clone(),
                        person: item.person.clone(),
                        summary: format!("Not sent: {tool} waited for an answer and expired. It is kept here as a draft."),
                        details: json!({ "tool": tool, "input": item.details["input"], "destinations": item.details["destinations"], "from_ask": item.id }),
                    },
                ),
            ];
            evs.extend(crate::inbox::reply(
                &run,
                &call_id,
                "No one answered in time, so this was not done. It is kept as a draft for the person. Do not retry it.",
                true,
                Label::PRINCIPAL,
            ));
            evs.extend(self.resume(&run));
            self.emit_all(evs).await?;
        }
        Ok(())
    }
}
