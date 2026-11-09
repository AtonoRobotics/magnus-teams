//! The loop contract (K2). Call the model, send every action through
//! `execute()`, write every step to the log. The loop holds no state: each
//! step starts from the log, so a new loop resumes any run.
//!
//! Code here only limits: it stops at the model-call and dollar budget (and
//! on a cancel, suspension or the kill switch) before each request, retries an
//! unavailable provider a bounded number of times, and suspends with a notice.
//! What to do next is always the model's decision.

use crate::boundary::{clamp_output_for_budget, estimated_input_tokens, Action, Issuer, Verdict};
use crate::events::{ContextRef, Ev};
use crate::grants::Spent;
use crate::kernel::Kernel;
use crate::state::Status;
use anyhow::Result;
use magnus_adapter_proto::ErrorClass;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// The longest wait between attempts at a failed model call (the backoff
/// below doubles from one second up to this).
const LONGEST_BACKOFF: Duration = Duration::from_millis(500 * 2u64.pow(6));

pub enum Flow {
    Continue,
    Stop,
}

impl Kernel {
    /// Whether the run may make another paid model request, read from the
    /// run as it is now. When not, the run is stopped the way the loop and
    /// the boundary's budget stage stop it, and nothing is asked.
    async fn may_ask_model(self: &Arc<Self>, run_id: &str) -> Result<bool> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(false) };
        // Cancelled, suspended or ended: whoever stopped it has said why.
        if run.status != Status::Running {
            return Ok(false);
        }
        if self.read(|s| s.kill_switch) {
            self.suspend(run_id, "kill_switch", None).await?;
            return Ok(false);
        }
        if self.read(|s| s.updating.is_some()) {
            self.suspend(run_id, "update", None).await?;
            return Ok(false);
        }
        let budget = &run.config.budget;
        if run.model_calls >= budget.max_model_calls {
            self.suspend_with_notice(
                run_id,
                "budget",
                format!("Stopped after {} model calls, the task's limit.", run.model_calls),
                json!({ "model_calls": run.model_calls, "limit": budget.max_model_calls, "usd": run.usd }),
            )
            .await?;
            return Ok(false);
        }
        // Dollars are spent by model calls, failed ones too: one past the
        // limit stops the run before the next is paid for.
        if run.usd >= budget.max_usd {
            let why = crate::boundary::budget_used(&run);
            self.suspend_with_notice(run_id, "budget", format!("Stopped: {why}."), json!({ "actions": run.actions, "usd": run.usd })).await?;
            return Ok(false);
        }
        // A task under a grant also stops once the grant's budget for its
        // period is spent, by all its tasks together (#280), until its
        // person resumes it, which is theirs to decide.
        if let Some(Spent { grant: g, spent, max, period }) = self.grant_spent(&run.id) {
            let why = format!("Stopped: the grant {g} has spent its ${max:.2} for this {period} (${spent:.2}).");
            self.suspend_with_notice(run_id, "grant_budget", why, json!({ "grant": g, "usd": spent, "limit": max })).await?;
            return Ok(false);
        }
        Ok(true)
    }

    /// The output tokens this call may write: the full allowance when its
    /// worst case fits what is left, fewer when only fewer fit, and none —
    /// the run stops for budget — when the prompt alone does not fit. A
    /// quote the adapter cannot give falls back to the full allowance: a
    /// down adapter charges nothing, and the call fails through the retry
    /// path below.
    async fn clamped_max_tokens(self: &Arc<Self>, run_id: &str, adapter: &str, model: &str, input_tokens: u64, max_tokens: u32) -> Result<Option<u32>> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(None) };
        let left = run.config.budget.max_usd - run.usd;
        let (input_usd, output_usd) = match self.adapters.quote(adapter, model, input_tokens, max_tokens).await {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("quote for {run_id}: {e:#}; asking without a worst-case check");
                return Ok(Some(max_tokens));
            }
        };
        match clamp_output_for_budget(left, input_usd, output_usd, max_tokens) {
            Some(n) => Ok(Some(n)),
            None => {
                let why = format!("Stopped: the next model call (about ${:.2} worst case) would pass the task's ${:.2} limit.", input_usd + output_usd, run.config.budget.max_usd);
                self.suspend_with_notice(run_id, "budget", why, json!({ "usd": run.usd, "limit": run.config.budget.max_usd, "next_call_usd": input_usd + output_usd })).await?;
                Ok(None)
            }
        }
    }

    pub async fn drive(self: Arc<Self>, run: &str) -> Result<()> {
        loop {
            match self.step(run).await? {
                Flow::Continue => continue,
                Flow::Stop => return Ok(()),
            }
        }
    }

    async fn step(self: &Arc<Self>, run_id: &str) -> Result<Flow> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(Flow::Stop) };
        if run.status != Status::Running {
            return Ok(Flow::Stop);
        }
        if self.read(|s| s.kill_switch) {
            self.suspend(run_id, "kill_switch", None).await?;
            return Ok(Flow::Stop);
        }
        // A person holds the computer, or has been asked to sign in: wait for them.
        if let Some(reason) = self.waits_on_person(&run) {
            self.suspend(run_id, reason, None).await?;
            return Ok(Flow::Stop);
        }
        // An update is switching releases: wait for the new one.
        if self.read(|s| s.updating.is_some()) {
            self.suspend(run_id, "update", None).await?;
            return Ok(Flow::Stop);
        }

        // Finish the tool calls of the latest reply, in order.
        if let Some((index, call_id, name, input)) = run.pending_calls().into_iter().next() {
            let step = run.replies.last().map(|r| r.step).unwrap_or(0);
            let step_id = format!("s{step}.{index}");
            let action = Action { run: run_id.into(), call_id, step_id, tool: name, input, issuer: Issuer::Model };
            let v = match self.watched(&run, &action).await? {
                Some(v) => v,
                None if crate::tools::RECORDS.contains(&action.tool.as_str()) => {
                    return self.record_call(&run, &action.call_id, &action.step_id, &action.tool, &action.input).await
                }
                None => self.execute(action).await?,
            };
            return Ok(match v {
                Verdict::Done { .. } => Flow::Continue,
                Verdict::Suspended => Flow::Stop,
            });
        }

        // A removable module: a run going in circles is reset, escalated, or stopped.
        if let Some(flow) = self.when_lost(&run).await? {
            return Ok(flow);
        }

        // Ask the model for the next step.
        if !self.may_ask_model(run_id).await? {
            return Ok(Flow::Stop);
        }
        // The tools first: asking an app for them is how Magnus learns it
        // refuses, which the context's app index then says.
        let tools = self.tools_for(&run).await;
        let (system, messages, relationship, personal) = self.context_with(&run).await?;
        let chat = self.is_chat_turn(&run);
        let tools_hash = {
            use sha2::Digest;
            hex::encode(&sha2::Sha256::digest(serde_json::to_vec(&tools)?)[..16])
        };
        let step = run.replies.len() as u32;
        let (adapter, model) = run.model();
        // The prompt's tokens, asked of the adapter once: what it would
        // send, not the kernel's bytes (#323). The quote below prices the
        // same prompt every attempt, while what is left is re-read. An
        // adapter that cannot measure falls back to the kernel's bytes.
        let input_tokens = match self.adapters.estimate_prompt(adapter, model, &system, messages.clone(), tools.clone()).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("prompt estimate for {run_id}: {e:#}; pricing the kernel's bytes");
                estimated_input_tokens(&system, &messages, &tools)
            }
        };
        let mut attempt = 0u32;
        loop {
            // Each attempt, a retry too, reads the run as it is now: a failed
            // call may have been charged, and a person may have cancelled or
            // the owner stopped everything while it waited.
            if !self.may_ask_model(run_id).await? {
                return Ok(Flow::Stop);
            }
            // The call's worst case against what is left: the prompt plus
            // the full output, at the model's prices. What is left bounds
            // the output; when the prompt alone does not fit, the run stops
            // for budget here, before anything is asked (#273).
            let max_tokens = match self.clamped_max_tokens(run_id, adapter, model, input_tokens, run.config.max_tokens).await? {
                Some(n) => n,
                None => return Ok(Flow::Stop),
            };
            self.emit(
                "kernel",
                Ev::ModelCall {
                    run: run_id.into(),
                    step,
                    attempt,
                    context: ContextRef {
                        through_seq: run.last_seq,
                        instructions: super::sent_version(&run.config.instructions, &system),
                        tools_hash: tools_hash.clone(),
                        relationship,
                        personal,
                        character: Some(super::character_version()),
                    },
                },
            )
            .await?;
            let live = self.live_text.clone();
            let rid = run_id.to_string();
            let out = self.adapters.complete(adapter, model, &system, messages.clone(), tools.clone(), max_tokens, |t| live.send(&rid, t)).await;
            live.end(&rid);
            match out {
                Ok(t) => {
                    let has_call = t.blocks.iter().any(|b| matches!(b, magnus_adapter_proto::Block::ToolCall { .. }));
                    let answer: String = t
                        .blocks
                        .iter()
                        .filter_map(|b| match b {
                            magnus_adapter_proto::Block::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();
                    if !has_call && self.cfg.safety.monitor_model.is_some() {
                        self.watch_answer(&run, &answer).await;
                    }
                    self.emit("model", Ev::ModelReply { run: run_id.into(), step, blocks: t.blocks, state: t.state, usage: t.usage, served_model: t.served_model, stop: t.stop })
                        .await?;
                    // A chat turn ends with its answer: no contract, no Done
                    // check. What it did on the way went through execute().
                    if !has_call && chat {
                        self.end_chat_turn(&run, answer).await?;
                        return Ok(Flow::Stop);
                    }
                    if !has_call {
                        self.emit(
                            "kernel",
                            Ev::Notice {
                                run: run_id.into(),
                                step: step + 1,
                                text: "Your reply called no tool, so nothing happened. Every reply must call at least one tool: continue the work, or call task_complete or task_fail.".into(),
                            },
                        )
                        .await?;
                    }
                    return Ok(Flow::Continue);
                }
                Err(e) => {
                    self.emit("kernel", Ev::ModelError { run: run_id.into(), step, attempt, class: e.class, message: e.message.clone(), usage: e.usage.clone() }).await?;
                    match e.class {
                        // An outage, or a rate limit the provider says will
                        // pass within the longest backoff, is waited out and
                        // asked again; a limit it says lasts longer (a spent
                        // quota) persists, and pauses the task below.
                        ErrorClass::Unavailable | ErrorClass::UsageLimit
                            if attempt < run.config.max_retries && (e.class == ErrorClass::Unavailable || e.retry_after_s.is_none_or(|s| s <= LONGEST_BACKOFF.as_secs_f64())) =>
                        {
                            attempt += 1;
                            let backoff = Duration::from_millis(500 * 2u64.pow(attempt.min(6))).min(LONGEST_BACKOFF);
                            let asked = e.retry_after_s.filter(|s| s.is_finite() && *s > 0.0).map(Duration::from_secs_f64).unwrap_or_default();
                            tokio::time::sleep(backoff.max(asked)).await;
                        }
                        ErrorClass::Malformed => {
                            self.emit("kernel", Ev::Notice { run: run_id.into(), step, text: format!("Your previous reply could not be used: {}. Reply again.", e.message) })
                                .await?;
                            return Ok(Flow::Continue);
                        }
                        class => {
                            let (reason, what) = match class {
                                ErrorClass::Unavailable => ("provider", "The model provider is unavailable"),
                                ErrorClass::UsageLimit => ("usage_limit", "The model provider's usage limit was reached"),
                                ErrorClass::AuthExpired => ("auth", "The model provider needs you to sign in again"),
                                _ => ("invalid_request", "The model provider rejected the request"),
                            };
                            // A rejection has many causes (billing, context
                            // too long): the person needs the provider's.
                            let why = if class == ErrorClass::InvalidRequest { format!(": {}", e.message.trim_end_matches('.')) } else { String::new() };
                            self.suspend_with_notice(
                                run_id,
                                reason,
                                format!("{what}{why}. The task is paused; resume it when this is fixed."),
                                json!({ "class": class, "message": e.message, "attempts": attempt + 1, "retry_after_s": e.retry_after_s }),
                            )
                            .await?;
                            return Ok(Flow::Stop);
                        }
                    }
                }
            }
        }
    }
}
