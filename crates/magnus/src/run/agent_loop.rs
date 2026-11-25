//! The loop contract (K2). Call the model, send every action through
//! `execute()`, write every step to the log. The loop holds no state: each
//! step starts from the log, so a new loop resumes any run.
//!
//! Code here only limits: it stops where the run's allowance says
//! (`spend::allowance`), or on a cancel, suspension or the kill switch, or
//! once a run above a sub-agent has ended (`run_end`),
//! before each request, retries an unavailable provider a bounded number of
//! times, and suspends with a notice.
//! What to do next is always the model's decision.

use crate::boundary::{clamp_output_for_budget, estimated_input_tokens, Action, Issuer, Verdict};
use crate::events::{ContextRef, Ev};
use crate::kernel::Kernel;
use crate::spend::allowance::{allowance_held, scope_of_run, Left, Spender};
use crate::spend::holds::{Held, Hold};
use crate::state::Status;
use anyhow::Result;
use chrono::Utc;
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
pub(super) const LIVE_TOOLS_FRESH: Duration = Duration::from_secs(30);
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

/// Why a step's call does not fit: the run may spend no more, or its worst
/// case is over what is left.
enum Unfit {
    Stop(Box<crate::spend::allowance::Stop>),
    Over(Left),
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
        // A sub-agent whose parent, or a run above it, has ended asks nothing more.
        if self.stopped_from_above(run_id).await? {
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

    /// The output tokens this call may write, and the hold on its worst
    /// case (`spend::holds`): the full allowance when its worst case fits
    /// what is left under every limit, less what other calls in flight
    /// hold; fewer when only fewer fit; and none — the run stops for the
    /// limit it would pass — when the prompt alone does not fit. A call
    /// that does not fit only for what others hold waits for them first.
    /// A quote the adapter cannot give stops the run: a call nobody can
    /// price would pass its limits unseen.
    async fn clamped_max_tokens<'k>(self: &'k Arc<Self>, run_id: &str, adapter: &str, model: &str, input_tokens: u64, max_tokens: u32) -> Result<Option<(u32, Held<'k>)>> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(None) };
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
        let scope = self.read(|s| scope_of_run(s, run_id));
        let fit = |held: &[Hold]| {
            let left = self.read(|s| allowance_held(s, run_id, Spender::Step, Utc::now(), held)).map_err(Unfit::Stop)?;
            match clamp_output_for_budget(left.usd, input_usd, output_usd, max_tokens) {
                Some(n) => Ok((n, input_usd + output_usd * f64::from(n) / f64::from(max_tokens.max(1)))),
                None => Err(Unfit::Over(left)),
            }
        };
        let worst = input_usd + output_usd;
        match self.spend_holds.admit(scope, fit).await {
            Ok(fits) => Ok(Some(fits)),
            Err(Unfit::Stop(stop)) => {
                self.suspend_with_notice(run_id, stop.reason, stop.notice, stop.details).await?;
                Ok(None)
            }
            Err(Unfit::Over(left)) if left.of == "task" => {
                let why = format!("Stopped: the next model call (about ${worst:.2} worst case) would pass the task's ${:.2} limit.", run.config.budget.max_usd);
                self.suspend_with_notice(run_id, "budget", why, json!({ "usd": run.spent_usd(), "limit": run.config.budget.max_usd, "next_call_usd": worst })).await?;
                Ok(None)
            }
            Err(Unfit::Over(left)) => {
                let why = format!("Stopped: the next model call (about ${worst:.2} worst case) would pass what is left of {} (${:.2}).", left.named(), left.usd);
                let reason = if left.of == "grant" { "grant_budget" } else { "spend_limit" };
                self.suspend_with_notice(
                    run_id,
                    reason,
                    why,
                    json!({ "which": left.of, "left_usd": left.usd, "next_call_usd": worst, "can_ask": left.of == "person", "pending": false }),
                )
                .await?;
                Ok(None)
            }
        }
    }

    /// The prompt's tokens as the adapter would send them, or the kernel's
    /// bytes when it cannot measure.
    async fn prompt_tokens(
        &self,
        run_id: &str,
        adapter: &str,
        model: &str,
        system: &str,
        messages: &[magnus_adapter_proto::Message],
        tools: &[magnus_adapter_proto::ToolSpec],
    ) -> u64 {
        match self.adapters.estimate_prompt(adapter, model, system, messages.to_vec(), tools.to_vec()).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("prompt estimate for {run_id} on {adapter}/{model}: {e:#}; pricing the kernel's bytes");
                estimated_input_tokens(system, messages, tools)
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
        // Nor does it act: the backstop holds before an action as before a model call.
        if self.stopped_from_above(run_id).await? {
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

        // Finish the tool calls of the latest reply: consecutive delegation
        // calls (`start_sub_agent`, `research_web`) and pure read-only tool
        // calls fan out concurrently so multi-source reads complete in parallel
        // while writes remain strictly sequential.
        let pending = run.pending_calls();
        if !pending.is_empty() {
            let step = run.replies.last().map(|r| r.step).unwrap_or(0);
            let is_parallelizable = |name: &str| {
                matches!(name, "start_sub_agent" | "research_web")
                    || (!crate::tools::RECORDS.contains(&name) && crate::boundary::facts(self, name).is_some_and(|f| f.risk == crate::tools::Risk::Read))
            };
            let parallel: Vec<_> = pending.iter().take_while(|(_, _, name, _)| is_parallelizable(name)).cloned().collect();
            if parallel.len() > 1 {
                let futs = parallel.into_iter().map(|(index, call_id, name, input)| {
                    let step_id = format!("s{step}.{index}");
                    let action = Action { run: run_id.into(), call_id, step_id, tool: name, input, issuer: Issuer::Model };
                    let k = self.clone();
                    let r = run.clone();
                    async move {
                        match k.watched(&r, &action).await? {
                            Some(v) => Ok(v),
                            None => k.execute(action).await,
                        }
                    }
                });
                let results = futures::future::join_all(futs).await;
                let mut suspended = false;
                for res in results {
                    if matches!(res?, Verdict::Suspended) {
                        suspended = true;
                    }
                }
                return Ok(if suspended { Flow::Stop } else { Flow::Continue });
            }
            if let Some((index, call_id, name, input)) = pending.into_iter().next() {
                let step_id = format!("s{step}.{index}");
                let input = self.take_approved_edit(run_id, &call_id).unwrap_or(input);
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
        let mut input_tokens = self.prompt_tokens(run_id, &active_adapter, &active_model, &system, &messages, &tools).await;
        let mut attempt = 0u32;
        loop {
            // Each attempt, a retry too, reads the run as it is now: a failed
            // call may have been charged, and a person may have cancelled or
            // the owner stopped everything while it waited.
            if self.may_ask_model(run_id).await?.is_none() {
                return Ok(Flow::Stop);
            }
            // A provider an admin turned off is asked nothing: the run goes
            // on with a fallback it may use, or pauses (`spend::paid`).
            if let Err(why) = crate::spend::paid::provider_on(self, &active_adapter) {
                match self.next_fallback(&run.person, (&active_adapter, &active_model), &mut fallback_idx).await {
                    Some((a, m)) => {
                        (active_adapter, active_model) = (a, m);
                        input_tokens = self.prompt_tokens(run_id, &active_adapter, &active_model, &system, &messages, &tools).await;
                        continue;
                    }
                    None => {
                        self.suspend_with_notice(
                            run_id,
                            "provider",
                            format!("Stopped: {why}. The task is paused; resume it when an admin turns it on."),
                            json!({ "adapter": active_adapter }),
                        )
                        .await?;
                        return Ok(Flow::Stop);
                    }
                }
            }
            // The call's worst case against what is left: the prompt plus
            // the full output, at the model's prices. What is left under
            // every limit bounds the output; when the prompt alone does not
            // fit, the run stops here, before anything is asked (#273).
            let (max_tokens, held) = match self.clamped_max_tokens(run_id, &active_adapter, &active_model, input_tokens, run.config.max_tokens).await? {
                Some(fits) => fits,
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
                    self.emit("model", Ev::ModelReply { run: run_id.into(), step, blocks: t.blocks, state: t.state, usage: t.usage, served_model: t.served_model, stop: t.stop })
                        .await?;
                    // Its cost is on the run now: what it held is free for
                    // the monitor's own call.
                    drop(held);
                    if !has_call && self.cfg.safety.monitor_model.is_some() {
                        self.watch_answer(&run, &answer).await;
                    }
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
                    drop(held);
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
                            // The next fallback the person may use whose
                            // provider is on (`spend::paid`).
                            if matches!(class, ErrorClass::Unavailable | ErrorClass::UsageLimit) {
                                if let Some((a, m)) = self.next_fallback(&run.person, (&active_adapter, &active_model), &mut fallback_idx).await {
                                    (active_adapter, active_model) = (a, m);
                                    input_tokens = self.prompt_tokens(run_id, &active_adapter, &active_model, &system, &messages, &tools).await;
                                    attempt += 1;
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
