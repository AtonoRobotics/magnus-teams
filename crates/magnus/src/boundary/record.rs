//! How a call's outcome is recorded: the one way tool output becomes what
//! the model sees. Every result, failure and denial passes `record_from`,
//! which hands the text to `Kernel::planner_text` (quarantine_runtime): in
//! a quarantined run, untrusted output reaches the planner only behind an
//! opaque handle, error or not (2026-11-23, one way tool output reaches
//! the model).

use super::{facts, Action, Issuer, Verdict};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::Label;
use anyhow::Result;
use serde_json::Value;

/// Every model-facing error says what failed and whether a retry can help (K8).
pub(super) fn err_text(what: &str, retry: bool) -> String {
    if retry {
        format!("{what} A retry may help.")
    } else {
        format!("{what} A retry of the same request will not help.")
    }
}

pub(super) struct Record<'a> {
    pub status: &'a str,
    pub text: String,
    pub is_error: bool,
    /// Who wrote the text: the tool's label when the app's words are in it,
    /// the kernel's own (`PRINCIPAL`) when only the kernel's are.
    pub label: Label,
    pub data: Option<Value>,
}

impl Kernel {
    pub(super) async fn record(&self, a: &Action, key: Option<String>, r: Record<'_>) -> Result<Verdict> {
        let sources = facts(self, &a.tool).map(|f| vec![f.app]).unwrap_or_default();
        self.record_from(a, key, sources, r).await
    }

    pub(super) async fn record_from(&self, a: &Action, key: Option<String>, sources: Vec<String>, r: Record<'_>) -> Result<Verdict> {
        let (text, data) = self.screen_inbound(&a.run, &a.tool, r.text, r.data, r.label.untrusted).await;
        let (text, data) = self.planner_text(&a.run, &a.tool, text, data, r.label, r.is_error).await;
        // What the verifier reads taints the run like the model's own reads:
        // its words go back to the model.
        self.emit(
            "kernel",
            Ev::ToolResult {
                run: a.run.clone(),
                call_id: a.call_id.clone(),
                key,
                status: r.status.into(),
                text: text.clone(),
                is_error: r.is_error,
                label: r.label,
                data: data.clone(),
                sources,
                check: a.issuer == Issuer::ReadBack,
            },
        )
        .await?;
        Ok(Verdict::Done { text, is_error: r.is_error, data })
    }

    pub(crate) async fn decision(&self, a: &Action, outcome: &str, stage: &str, reason: &str, rule: Option<String>) -> Result<()> {
        self.emit(
            "kernel",
            Ev::Decision(crate::events::DecisionPayload {
                run: a.run.clone(),
                call_id: a.call_id.clone(),
                tool: a.tool.clone(),
                outcome: outcome.into(),
                stage: stage.into(),
                reason: reason.into(),
                rule,
            }),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn deny(&self, a: &Action, stage: &str, reason: &str, retry: bool, rule: Option<String>) -> Result<Verdict> {
        self.deny_as(a, stage, reason, retry, rule, Label::PRINCIPAL).await
    }

    /// `deny`, for a reason that carries words of the app's (what it
    /// answered a read the gate made): recorded under the app's `label`, so
    /// a quarantined run sees them only behind a handle.
    pub(super) async fn deny_as(&self, a: &Action, stage: &str, reason: &str, retry: bool, rule: Option<String>, label: Label) -> Result<Verdict> {
        self.decision(a, "deny", stage, reason, rule).await?;
        let text = err_text(&format!("Denied ({stage}): {reason}."), retry);
        self.record(a, None, Record { status: "denied", text, is_error: true, label, data: None }).await
    }
}
