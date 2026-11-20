//! The loop contract (K2). Call the model, send every action through
//! `execute()`, write every step to the log. The loop holds no state: each
//! step starts from the log, so a new loop resumes any run.
//!
//! Code here only limits: it stops where the run's allowance says
//! (`spend::allowance`), or on a cancel, suspension or the kill switch,
//! before each request, retries an unavailable provider a bounded number of
//! times, and suspends with a notice.
//! What to do next is always the model's decision.

use crate::boundary::{clamp_output_for_budget, estimated_input_tokens, Action, Issuer, Verdict};
use crate::events::{ContextRef, Ev};
use crate::kernel::Kernel;
use crate::spend::allowance::{Left, Spender};
use crate::state::Status;
use anyhow::Result;
use magnus_adapter_proto::ErrorClass;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Half the first wait between attempts at a failed model call: the backoff
/// doubles from one second.
const BACKOFF_BASE: Duration = Duration::from_millis(500);

/// The most doublings of [`BACKOFF_BASE`].
const MOST_BACKOFF_DOUBLINGS: u32 = 6;

/// The longest wait between attempts at a failed model call (the backoff
/// below doubles from one second up to this).
/// How long an app's live tool list is reused before it is asked again.
pub(super) const LIVE_TOOLS_FRESH: Duration = Duration::from_secs(2);
const LONGEST_BACKOFF: Duration = Duration::from_millis(500 * 2u64.pow(MOST_BACKOFF_DOUBLINGS));

/// How long to wait before attempt `next_attempt` (1 for the first retry)
/// of a model call that failed as unavailable or over a usage limit, or
/// `None` when the provider says it will not pass within
/// [`LONGEST_BACKOFF`]: the task pauses then instead of waiting, whatever
/// the class, so no provider's answer can hold a run asleep. An answer
/// that is not a positive number is no answer.
pub(crate) fn retry_delay(next_attempt: u32, retry_after_s: Option<f64>) -> Option<Duration> {
    let backoff = (BACKOFF_BASE * 2u32.pow(next_attempt.min(MOST_BACKOFF_DOUBLINGS))).min(LONGEST_BACKOFF);
    match retry_after_s {
        Some(s) if s > LONGEST_BACKOFF.as_secs_f64() => None,
        Some(s) if s > 0.0 => Some(backoff.max(Duration::from_secs_f64(s)).min(LONGEST_BACKOFF)),
        _ => Some(backoff),
    }
}

pub enum Flow {
    Continue,
    Stop,
}

impl Kernel {
    /// What the run has left for another paid model request, read from the
    /// run as it is now. When it may not ask, the run is stopped the way
    /// its allowance says (`spend::allowance`), and nothing is asked.
    async fn may_ask_model(self: &Arc<Self>, run_id: &str) -> Result<Option<Left>> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(None) };
        // Cancelled, suspended or ended: whoever stopped it has said why.
        if run.status != Status::Running {
            return Ok(None);
        }
        if let Some((reason, _)) = self.halt_of(run_id, None) {
            self.suspend(run_id, &reason, None).await?;
            return Ok(None);
        }
        if self.read(|s| s.server_ops.updating().is_some()) {
            self.suspend(run_id, "update", None).await?;
            return Ok(None);
        }
        match self.allowance(run_id, Spender::Step) {
            Ok(left) => Ok(Some(left)),
            Err(stop) => {
                self.suspend_with_notice(run_id, stop.reason, stop.notice, stop.details).await?;
                Ok(None)
            }
        }
    }

    /// The output tokens this call may write: the full allowance when its
    /// worst case fits what is left, fewer when only fewer fit, and none —
    /// the run stops for budget — when the prompt alone does not fit. A
    /// quote the adapter cannot give falls back to the full allowance: a
    /// down adapter charges nothing, and the call fails through the retry
    /// path below.
    #[allow(clippy::too_many_arguments)]
    async fn clamped_max_tokens(self: &Arc<Self>, run_id: &str, left: &Left, adapter: &str, model: &str, input_tokens: u64, max_tokens: u32) -> Result<Option<u32>> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(None) };
        let left = left.usd;
        let (input_usd, output_usd) = match self.adapters.quote(adapter, model, input_tokens, max_tokens).await {
            Ok(q) => q,
            // Fail closed: a call nobody can price would pass the task's
            // limit unseen ("Its budget is the server's per-task limit").
            Err(e) => {
                let why = format!("Stopped: the next model call could not be priced, so it could not be counted against the task's ${:.2} limit.", run.config.budget.max_usd);
                self.suspend_with_notice(
                    run_id,
                    "budget",
                    why,
                    json!({ "usd": run.spent_usd(), "limit": run.config.budget.max_usd, "model": model, "unpriced": format!("{e:#}") }),
                )
                .await?;
                return Ok(None);
            }
        };
        match clamp_output_for_budget(left, input_usd, output_usd, max_tokens) {
            Some(n) => Ok(Some(n)),
            None => {
                let why = format!("Stopped: the next model call (about ${:.2} worst case) would pass the task's ${:.2} limit.", input_usd + output_usd, run.config.budget.max_usd);
                self.suspend_with_notice(run_id, "budget", why, json!({ "usd": run.spent_usd(), "limit": run.config.budget.max_usd, "next_call_usd": input_usd + output_usd }))
                    .await?;
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
        if let Some((reason, _)) = self.halt_of(run_id, None) {
            self.suspend(run_id, &reason, None).await?;
            return Ok(Flow::Stop);
        }
        // A person holds the computer, or has been asked to sign in: wait for them.
        if let Some(reason) = self.waits_on_person(&run) {
            self.suspend(run_id, reason, None).await?;
            return Ok(Flow::Stop);
        }
        // An update is switching releases: wait for the new one.
        if self.read(|s| s.server_ops.updating().is_some()) {
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
        if self.may_ask_model(run_id).await?.is_none() {
            return Ok(Flow::Stop);
        }
        // The tools first: asking an app for them is how Magnus learns it
        // refuses, which the context's app index then says.
        let tools = self.tools_for(&run).await;
        let (system, messages, relationship, personal) = self.context_with(&run).await?;
        let chat = self.run_kind(&run.id).converses();
        let tools_hash = {
            use sha2::Digest;
            hex::encode(&sha2::Sha256::digest(serde_json::to_vec(&tools)?)[..16])
        };
        let step = run.replies.len() as u32;
        let (primary_adapter, primary_model) = run.model();
        let mut active_adapter = primary_adapter.to_string();
        let mut active_model = primary_model.to_string();
        let mut fallback_idx = 0usize;
        // The prompt's tokens, asked of the adapter once: what it would
        // send, not the kernel's bytes (#323). The quote below prices the
        // same prompt every attempt, while what is left is re-read. An
        // adapter that cannot measure falls back to the kernel's bytes.
        let mut input_tokens = match self.adapters.estimate_prompt(&active_adapter, &active_model, &system, messages.clone(), tools.clone()).await {
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
            let Some(left) = self.may_ask_model(run_id).await? else {
                return Ok(Flow::Stop);
            };
            // The call's worst case against what is left: the prompt plus
            // the full output, at the model's prices. What is left bounds
            // the output; when the prompt alone does not fit, the run stops
            // for budget here, before anything is asked (#273).
            let max_tokens = match self.clamped_max_tokens(run_id, &left, &active_adapter, &active_model, input_tokens, run.config.max_tokens).await? {
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
            let out = self
                .adapters
                .complete_with_reasoning(&active_adapter, &active_model, &system, messages.clone(), tools.clone(), max_tokens, run.config.reasoning.as_deref(), |t| {
                    live.send(&rid, t)
                })
                .await;
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
                    // A block that cannot be drawn goes back to be fixed.
                    let files: Vec<String> = run.outputs.iter().map(|(_, n, ..)| n.clone()).collect();
                    if let Some(why) = crate::ui::docs::problems(&answer, &run.person, &files) {
                        let text = format!("A block in your reply cannot be drawn: {why}. Write it again, or use ui_catalog to see how the element is written.");
                        self.emit("kernel", Ev::Notice { run: run_id.into(), step: step + 1, text }).await?;
                        return Ok(Flow::Continue);
                    }
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
                    let wait = retry_delay(attempt + 1, e.retry_after_s);
                    match e.class {
                        // An outage or a rate limit the provider says will
                        // pass within the longest backoff is waited out and
                        // asked again; one it says lasts longer (a spent
                        // quota, a long outage) persists, and pauses the
                        // task below.
                        ErrorClass::Unavailable | ErrorClass::UsageLimit if attempt < run.config.max_retries && wait.is_some() => {
                            attempt += 1;
                            tokio::time::sleep(wait.unwrap_or(LONGEST_BACKOFF)).await;
                        }
                        ErrorClass::Malformed => {
                            self.emit("kernel", Ev::Notice { run: run_id.into(), step, text: format!("Your previous reply could not be used: {}. Reply again.", e.message) })
                                .await?;
                            return Ok(Flow::Continue);
                        }
                        class => {
                            if matches!(class, ErrorClass::Unavailable | ErrorClass::UsageLimit) {
                                let allowed = self.allowed_models(&run.person).await;
                                let mut switched = false;
                                while let Some(entry) = self.cfg.model.fallbacks.get(fallback_idx) {
                                    fallback_idx += 1;
                                    if let Some((fa, fm)) = entry.split_once('/').map(|(a, m)| (a.trim(), m.trim())).filter(|(a, m)| !a.is_empty() && !m.is_empty()) {
                                        if (fa != active_adapter || fm != active_model) && crate::spend::access::permits(&allowed, fa, fm) {
                                            active_adapter = fa.to_string();
                                            active_model = fm.to_string();
                                            input_tokens = match self.adapters.estimate_prompt(&active_adapter, &active_model, &system, messages.clone(), tools.clone()).await {
                                                Ok(n) => n,
                                                Err(err) => {
                                                    tracing::warn!("prompt estimate for {run_id} on fallback {active_adapter}/{active_model}: {err:#}; pricing the kernel's bytes");
                                                    estimated_input_tokens(&system, &messages, &tools)
                                                }
                                            };
                                            attempt += 1;
                                            switched = true;
                                            break;
                                        }
                                    }
                                }
                                if switched {
                                    continue;
                                }
                            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unbounded_retry_after_pauses_instead_of_sleeping() {
        assert_eq!(retry_delay(1, Some(1e12)), None);
        assert_eq!(retry_delay(1, Some(f64::INFINITY)), None);
        assert_eq!(retry_delay(1, Some(LONGEST_BACKOFF.as_secs_f64() + 1.0)), None);
    }

    #[test]
    fn a_retry_after_within_the_longest_backoff_is_waited() {
        assert_eq!(retry_delay(1, Some(20.0)), Some(Duration::from_secs(20)));
        assert_eq!(retry_delay(1, Some(LONGEST_BACKOFF.as_secs_f64())), Some(LONGEST_BACKOFF));
    }

    #[test]
    fn no_usable_answer_backs_off_by_doubling_up_to_the_longest() {
        assert_eq!(retry_delay(1, None), Some(Duration::from_secs(1)));
        assert_eq!(retry_delay(2, Some(f64::NAN)), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(3, Some(-5.0)), Some(Duration::from_secs(4)));
        for n in 0..100 {
            assert!(retry_delay(n, None).is_some_and(|d| d <= LONGEST_BACKOFF));
        }
        assert_eq!(retry_delay(u32::MAX, None), Some(LONGEST_BACKOFF));
    }
}
