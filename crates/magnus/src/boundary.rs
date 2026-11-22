//! The execution boundary (K3, B1): the single path between the agent and
//! anything outside its own context. Every action passes every stage, in order:
//! identify, lease, budget, policy, approval, idempotency, credentials,
//! execute. The first non-allow outcome ends the pipeline. Any stage error
//! denies (fail closed). The intent is recorded before the external call and
//! the result after (intent first). Credentials are resolved last and never
//! leave the call.

mod call;

pub(crate) use call::memory_text;
pub use call::{computer_command, media_type_for};
use call::{memory_taint, CallFailure};

use crate::config::{Connector, Person};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::policy::{self, Outcome, ToolFacts};
use crate::run::admission::{Refusal, HARD_DENIAL};
use crate::spend::allowance::Spender;
use crate::state::{Intent, Run};
use crate::tool_registry::Taint;
use crate::tools::{self, Audience, Label, Risk};
use crate::vault::redact;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issuer {
    /// The model asked for it.
    Model,
    /// The server reading the world back after an irreversible action
    /// (spec rev 147, `read_back.rs`). It may only read.
    ReadBack,
    /// The memory pass filing what the exchange taught about the person; it
    /// may only write memory.
    Memory,
}

impl Issuer {
    fn as_str(self) -> &'static str {
        match self {
            Issuer::Model => "model",
            Issuer::ReadBack => "kernel:read_back",
            Issuer::Memory => "kernel:memory_pass",
        }
    }

    /// Reads only, and never waits for approval.
    pub(crate) fn reads_only(self) -> bool {
        matches!(self, Issuer::ReadBack)
    }
}

pub struct Action {
    pub run: String,
    /// The model's tool-call id, for matching the result; never the idempotency key.
    pub call_id: String,
    /// Position-derived step id (K32).
    pub step_id: String,
    pub tool: String,
    pub input: Value,
    pub issuer: Issuer,
}

pub enum Verdict {
    /// A result was recorded for the call.
    Done { text: String, is_error: bool, data: Option<Value> },
    /// The run was suspended (approval needed, budget, kill switch).
    Suspended,
}

/// What the boundary knows about a tool, from the owner's classification.
#[derive(Clone)]
pub struct Facts {
    pub name: String,
    pub app: String,
    pub risk: Risk,
    pub label: Label,
    pub audience: Audience,
    pub connector: Option<Connector>,
    pub remote: String,
    /// It stays in the person's own account (`tools::REACH_SELF`).
    pub reaches_self: bool,
}

impl<'a> From<&'a Facts> for ToolFacts<'a> {
    /// What a policy sees of a tool.
    fn from(f: &'a Facts) -> Self {
        ToolFacts { name: &f.name, app: &f.app, risk: f.risk, audience: f.audience }
    }
}

/// Look up a tool by the name the model uses.
pub fn facts(k: &Kernel, tool: &str) -> Option<Facts> {
    if let Some(b) = tools::builtin(tool) {
        // A team tool's change stays in its own workspace, which the whole
        // team sees: a post or a file there is not a send (owner, 2026-11-13).
        let reaches_self = b.app == "team" && b.risk.changes_state();
        return Some(Facts { name: tool.into(), app: b.app.into(), risk: b.risk, label: b.label, audience: b.audience, connector: None, remote: tool.into(), reaches_self });
    }
    let (app, remote) = tool.split_once("__")?;
    let Some(c) = k.connector(app) else { return crate::desktop::facts(app, remote) };
    let class = c.tools.get(remote)?;
    // Never the sandbox, which is the appliance's own: an older log may class
    // an app's tool so. A password manager's tools are the kernel's alone.
    let risk = Risk::parse(&class.risk).filter(|r| *r != Risk::Sandbox)?;
    let risk = if k.cfg.computer.as_ref().is_some_and(|c| c.password_manager.as_deref() == Some(app)) { Risk::Prohibited } else { risk };
    Some(Facts {
        name: tool.into(),
        app: app.into(),
        risk,
        reaches_self: class.reach.as_deref().is_some_and(|r| crate::tools::reach_fits(r, risk.as_str())),
        label: class.label.as_deref().and_then(Label::parse).or_else(|| Label::parse(&c.label))?,
        audience: Audience::parse(&c.audience)?,
        connector: Some(c),
        remote: remote.into(),
    })
}

/// The appliance's own chat tools (`create_task`, `create_schedule`, `suggest`,
/// `revoke_grant`, the task and schedule controls), never
/// a connector's, whatever the connector is called.
/// The rule on an approval card the send reviewer asked for: it flagged
/// the send, and the person decides (#179).
pub const REVIEWER_FLAG: &str = "reviewer:flag";

pub(crate) fn is_chat_tool(f: &Facts) -> bool {
    f.connector.is_none() && crate::run::admission::placement::is_chat_only(&f.name)
}

/// A send the reviewer judges: the model's own action, changing the world
/// outside Magnus. Its own memory, a chat message, and a local write are
/// not sends.
pub(crate) fn shown_hash(a: &Action, shown: Option<&Shown>) -> String {
    shown.map_or_else(|| input_hash(&a.tool, &a.input), |s| s.hash(&a.tool, &a.input))
}

pub(crate) fn is_reviewed_send(a: &Action, f: &Facts) -> bool {
    if a.issuer != Issuer::Model || is_chat_tool(f) || f.reaches_self || !f.risk.changes_state() {
        return false;
    }
    if f.connector.is_none() && f.app == "memory" {
        return false;
    }
    f.connector.is_some() || matches!(f.app.as_str(), "web" | "browser" | "computer" | "local" | "connector" | "team") || f.app.starts_with(crate::desktop::APP_PREFIX)
}

pub fn has_rights(person: &Person, f: &Facts) -> bool {
    f.connector.as_ref().is_none_or(|c| crate::policy::may_use_app(person, c))
}

/// The prompt's tokens from the kernel's bytes: the fallback when the
/// adapter cannot measure what it would send. The live path asks the
/// adapter, which drops what it cannot send.
pub(crate) fn estimated_input_tokens(system: &str, messages: &[magnus_adapter_proto::Message], tools: &[magnus_adapter_proto::ToolSpec]) -> u64 {
    let chars = system.len() + serde_json::to_string(messages).map(|s| s.len()).unwrap_or(0) + serde_json::to_string(tools).map(|s| s.len()).unwrap_or(0);
    magnus_adapter_proto::chars_to_tokens(chars)
}

/// The output tokens the dollars left allow: the full allowance when the
/// quoted worst case fits, fewer when only fewer fit, none when the
/// prompt alone does not fit. The quote scales linearly in output tokens
/// (the adapter contract), so what is left over the prompt prices the
/// headroom.
pub(crate) fn clamp_output_for_budget(remaining_usd: f64, input_usd: f64, output_usd_full: f64, max_tokens: u32) -> Option<u32> {
    if input_usd > remaining_usd {
        return None;
    }
    if output_usd_full <= 0.0 {
        return Some(max_tokens);
    }
    let unit = output_usd_full / max_tokens.max(1) as f64;
    if unit <= 0.0 {
        return Some(max_tokens);
    }
    let affordable = ((remaining_usd - input_usd) / unit).floor().clamp(0.0, max_tokens as f64) as u32;
    if affordable < 1 {
        None
    } else {
        Some(affordable)
    }
}

pub fn idempotency_key(run: &str, step_id: &str, tool: &str) -> String {
    use sha2::Digest;
    hex::encode(&sha2::Sha256::digest(format!("{run}\n{step_id}\n{tool}").as_bytes())[..16])
}

/// JSON with object keys sorted at every level, so a value read back from the
/// log (jsonb reorders keys) hashes the same as the value first seen.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&m[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(canonical).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

pub fn input_hash(tool: &str, input: &Value) -> String {
    use sha2::Digest;
    hex::encode(&sha2::Sha256::digest(format!("{tool}\n{}", canonical(input)).as_bytes())[..16])
}

/// Where an action goes, read at the gate from what it acts on (approvals
/// spec, Destination fields): each recipients field's addresses and each
/// content field, by field name. Its card shows them, and an approval holds
/// only for exactly these.
pub struct Shown {
    pub recipients: std::collections::BTreeMap<String, Vec<String>>,
    pub content: std::collections::BTreeMap<String, Value>,
    /// What the approval also holds for, unshown (`bound`, #177).
    pub bound: std::collections::BTreeMap<String, Value>,
}

impl Shown {
    pub fn details(&self) -> Value {
        json!({ "recipients": self.recipients, "content": self.content })
    }

    /// The approval hash of this action going to exactly these recipients
    /// with exactly this content, every part of it: a draft changed after
    /// it was approved, even by one added Bcc or only in its HTML part, is
    /// another action and asks again.
    pub fn hash(&self, tool: &str, input: &Value) -> String {
        use sha2::Digest;
        let held = json!({ "shown": self.details(), "bound": self.bound });
        hex::encode(&sha2::Sha256::digest(format!("{tool}\n{}\n{}", canonical(input), canonical(&held)).as_bytes())[..16])
    }
}

/// Every model-facing error says what failed and whether a retry can help (K8).
fn err_text(what: &str, retry: bool) -> String {
    if retry {
        format!("{what} A retry may help.")
    } else {
        format!("{what} A retry of the same request will not help.")
    }
}

pub(crate) enum Gate {
    Pass,
    Stop(Verdict),
}

impl Kernel {
    async fn record(&self, a: &Action, key: Option<String>, status: &str, text: String, is_error: bool, label: Label, data: Option<Value>) -> Result<Verdict> {
        let sources = facts(self, &a.tool).map(|f| vec![f.app]).unwrap_or_default();
        self.record_from(a, key, status, text, is_error, label, sources, data).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_from(
        &self,
        a: &Action,
        key: Option<String>,
        status: &str,
        text: String,
        is_error: bool,
        label: Label,
        sources: Vec<String>,
        data: Option<Value>,
    ) -> Result<Verdict> {
        let (text, data) = self.screen_inbound(&a.run, &a.tool, text, data, label.untrusted).await;
        let text = if label.untrusted && !is_error && self.run_is_quarantined(&a.run) {
            if let Some(run) = self.read(|s| s.runs.get(&a.run).cloned()) {
                self.quarantine_tool_output(&run, &a.tool, &text).await
            } else {
                text
            }
        } else {
            text
        };
        // What the verifier reads taints the run like the model's own reads:
        // its words go back to the model.
        self.emit(
            "kernel",
            Ev::ToolResult {
                run: a.run.clone(),
                call_id: a.call_id.clone(),
                key,
                status: status.into(),
                text: text.clone(),
                is_error,
                label,
                data: data.clone(),
                sources,
                check: a.issuer == Issuer::ReadBack,
            },
        )
        .await?;
        Ok(Verdict::Done { text, is_error, data })
    }

    pub(crate) async fn decision(&self, a: &Action, outcome: &str, stage: &str, reason: &str, rule: Option<String>) -> Result<()> {
        self.emit(
            "kernel",
            Ev::Decision { run: a.run.clone(), call_id: a.call_id.clone(), tool: a.tool.clone(), outcome: outcome.into(), stage: stage.into(), reason: reason.into(), rule },
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn deny(&self, a: &Action, stage: &str, reason: &str, retry: bool, rule: Option<String>) -> Result<Verdict> {
        self.decision(a, "deny", stage, reason, rule).await?;
        let text = err_text(&format!("Denied ({stage}): {reason}."), retry);
        self.record(a, None, "denied", text, true, Label::PRINCIPAL, None).await
    }

    /// Run an action through the boundary. Errors inside any stage deny it.
    pub async fn execute(self: &Arc<Self>, a: Action) -> Result<Verdict> {
        match self.pipeline(&a).await {
            Ok(v) => Ok(v),
            Err(e) => {
                // Fail closed. If even the denial cannot be recorded, the error
                // propagates and the run stops until the log is writable.
                let reason = format!("internal error: {e:#}");
                tracing::warn!("boundary {}: {reason}", a.tool);
                self.deny(&a, "boundary", &reason, true, None).await
            }
        }
    }

    async fn pipeline(self: &Arc<Self>, a: &Action) -> Result<Verdict> {
        // 1. Identify: the run, the person and the tool are known.
        let Some(run) = self.read(|s| s.runs.get(&a.run).cloned()) else { return Err(anyhow!("unknown run {}", a.run)) };
        let person = self.person(&run.person).ok_or_else(|| anyhow!("unknown person {}", run.person))?;
        // The kill switch holds before anything touches a credential (B11).
        if let Some((reason, why)) = self.halt_of(&a.run, Some(&a.tool)) {
            self.decision(a, "deny", "identify", &why, None).await?;
            self.suspend(&a.run, &reason, None).await?;
            return Ok(Verdict::Suspended);
        }
        let Some(f) = facts(self, &a.tool) else {
            return self.deny(a, "identify", &format!("{} is not a tool this server can use (it is unknown or the owner has not classified it)", a.tool), false, None).await;
        };
        if let Some(c) = &f.connector {
            // Compare the live definition with the pin right before acting, so
            // a definition changed after it was listed cannot run (B10).
            let person = self.app_owner(&run, c);
            if !self.connected(c, &person) {
                return self
                    .deny(
                        a,
                        "credentials",
                        &format!(
                            "{} is not connected for the person this task is for: they connect it with their own account in the Library, and no one else's sign-in is used",
                            f.app
                        ),
                        false,
                        None,
                    )
                    .await;
            }
            if let Err((why, retry)) = self.check_pin(c, &f, &person, &a.run).await {
                return self.deny(a, "identify", &why, retry, None).await;
            }
        }

        // 2. Lease: one run at a time writes a shared record.
        if f.risk.changes_state() {
            if let Gate::Stop(v) = self.lease(a, &f).await? {
                return Ok(v);
            }
        }

        // 3. Budget: the run's allowance for an action, its grant's, the
        // month's and its team's limits with it (`spend::allowance`).
        if a.issuer == Issuer::Model {
            if let Err(stop) = self.allowance(&a.run, Spender::Action) {
                self.decision(a, "ask", "budget", &stop.why, None).await?;
                self.suspend_with_notice(&a.run, stop.reason, stop.notice, stop.details).await?;
                return Ok(Verdict::Suspended);
            }
        }

        // Where it goes, read before anything is decided; none of it reaches the model.
        let shown = match self.destinations(a, &f, &run, &person).await {
            Ok(s) => s,
            Err(why) => return self.deny(a, "destinations", &why, true, None).await,
        };

        // 4. Policy denials, the person's rules (block, ask first), and the kernel's asks: a payment, case 3.
        let policy = self.policy_stage(a, &run, &person, &f, shown.as_ref()).await?;
        let asked = match self.person_rules(a, &run, &f, shown.as_ref(), policy).await? {
            Ok(ask) => ask.map(|(why, rule)| (why, rule.unwrap_or_default())),
            Err(v) => return Ok(v),
        };

        // 5. The person's answer, one of the four asks, or Auto Review.
        if let Gate::Stop(v) = self.settle(a, &run, &f, shown.as_ref(), asked).await? {
            return Ok(v);
        }

        // 6. Idempotency.
        let key = idempotency_key(&a.run, &a.step_id, &a.tool);
        let existing = self.read(|s| s.runs.get(&a.run).and_then(|r| r.intents.get(&key).cloned()));
        let resend = match existing {
            Some(Intent { input, result: Some(prev), .. }) => {
                if input != a.input {
                    return self.deny(a, "idempotency", "this step was already used for a different request", false, None).await;
                }
                self.decision(a, "dedupe", "idempotency", "already done", None).await?;
                return self.record(a, Some(key), &prev.status, prev.text, prev.is_error, f.label, prev.data).await;
            }
            Some(Intent { input, result: None, .. }) => {
                if input != a.input {
                    return self.deny(a, "idempotency", "this step was already used for a different request", false, None).await;
                }
                // A crash left this intent without a result.
                match self.reconcile(a, &f, &key).await? {
                    Gate::Pass => true,
                    Gate::Stop(v) => return Ok(v),
                }
            }
            None => false,
        };

        // 7. Credentials, last.
        if let Some((_, why)) = self.halt_of(&a.run, Some(&a.tool)) {
            return self.deny(a, "credentials", &why, false, None).await;
        }
        // The person's own sign-in, or in a team thread the workspace's, never anyone else's.
        let person = f.connector.as_ref().map_or_else(|| run.person.clone(), |c| self.app_owner(&run, c));
        // Taken before the sign-in is: a brokered call that waits while its
        // microVM is stopped (Disconnect, deletion, the kill switch) sees it
        // was, and sends nothing with the token it holds.
        let stops = f.connector.as_ref().map_or(0, |c| self.brokered.generation(&c.name, &person));
        let secret = match f.connector.as_ref().filter(|c| c.credential.is_some()) {
            Some(c) => match self.connector_secret(c, &person).await? {
                Some(s) => Some(s),
                None => {
                    let why = if self.lapsed(c, &person) {
                        format!("{} no longer accepts the sign-in of the person this task is for: offer_app puts its card in front of them to sign in again", f.app)
                    } else {
                        format!("{} is not connected for the person this task is for (or their sign-in expired): they sign in again in the Library, or offer_app puts its card in front of them", f.app)
                    };
                    return self.deny(a, "credentials", &why, false, None).await;
                }
            },
            None => None,
        };

        // 8. Execute: intent first, then the call, then the result. After an
        // irreversible action the server reads the world back (spec rev 147).
        if !resend {
            self.emit(
                "kernel",
                Ev::Intent {
                    run: a.run.clone(),
                    call_id: a.call_id.clone(),
                    step_id: a.step_id.clone(),
                    key: key.clone(),
                    tool: a.tool.clone(),
                    input: a.input.clone(),
                    issuer: a.issuer.as_str().into(),
                },
            )
            .await?;
        }
        // memory_search returns content with the taint it was written under
        // (M1), taken from the same entries it returns: a second lookup could
        // miss a fact forgotten in between and label its text clean.
        let taint = crate::tool_registry::taint(&a.tool);
        let (out, memory) = if taint == Taint::Memory {
            let hits = self.memory_hits(&a.run, a.input["query"].as_str().unwrap_or_default()).await?;
            (Ok((memory_text(&hits), false, None)), Some(memory_taint(&hits)))
        } else {
            (self.call(a, &f, secret.as_ref().map(|s| s.expose()), &key, stops).await, None)
        };
        let (label, sources) = if let Some(taint) = memory {
            taint
        } else {
            let d = || out.as_ref().ok().and_then(|(_, _, d)| d.clone()).unwrap_or_default();
            match taint {
                // What the search returned decides; a result that says nothing is untrusted and private.
                Taint::Returned => (Label { untrusted: d()["untrusted"] != false, private: d()["private"] != false }, vec![f.app.clone()]),
                // A reference file carries its own taint (M1, M2).
                Taint::ProjectFile => (self.project_file(&a.run, a.input["name"].as_str().unwrap_or_default()).map_or(Label::PRINCIPAL, |f| f.label), vec![f.app.clone()]),
                // The summaries it lists carry what their runs read.
                Taint::Listed if is_chat_tool(&f) => self.listed_taint(&d()),
                _ => (f.label, vec![f.app.clone()]),
            }
        };
        let clean = |t: &str| match &secret {
            Some(s) => redact(t, s.expose()),
            None => t.to_string(),
        };
        match out {
            Ok((text, is_error, data)) => {
                let filter = |t: String| if f.connector.as_ref().is_some_and(|c| c.filter_login_codes) { crate::login_codes::strip(&t) } else { t };
                let (text, data) = crate::attachment::lift(&a.run, f.connector.as_ref().is_some_and(|c| c.filter_login_codes), text, data, |t| filter(clean(&t)));
                let text = match crate::read_back::applies(a, &f, is_error) {
                    true => format!("{text}\n\n{}", self.read_back(a, &f, shown.as_ref(), data.as_ref()).await?),
                    false => text,
                };
                self.record_from(a, Some(key), if is_error { "failed" } else { "done" }, text, is_error, label, sources, data).await
            }
            Err(CallFailure::NotDone(m)) => self.record(a, Some(key), "failed", err_text(&format!("{} failed: {}.", a.tool, clean(&m)), true), true, f.label, None).await,
            // A resend held back: the attempt before it may have reached
            // the app, so its outcome stays unknown, never "not sent".
            Err(CallFailure::Stopped(m)) if resend => {
                let text = err_text(&format!("{}: an earlier attempt may have reached {}, so its outcome is unknown; it was not sent again: {}.", a.tool, f.app, clean(&m)), false);
                self.record(a, Some(key), "unknown", text, true, f.label, None).await
            }
            Err(CallFailure::Stopped(m)) => self.record(a, Some(key), "failed", err_text(&format!("{} was not sent: {}.", a.tool, clean(&m)), false), true, f.label, None).await,
            Err(CallFailure::Unknown(m)) => {
                // The request may have reached the app. Resolve as after a crash.
                tracing::warn!("{}: outcome unknown: {m}", a.tool);
                match self.reconcile(a, &f, &key).await? {
                    Gate::Pass => {
                        let again = self.call(a, &f, secret.as_ref().map(|s| s.expose()), &key, stops).await;
                        match again {
                            Ok((text, is_error, data)) => {
                                let data = data.map(|d| serde_json::from_str(&clean(&d.to_string())).unwrap_or(Value::Null));
                                self.record(a, Some(key), if is_error { "failed" } else { "done" }, clean(&text), is_error, f.label, data).await
                            }
                            Err(e) => {
                                let m = match e {
                                    CallFailure::NotDone(m) | CallFailure::Unknown(m) | CallFailure::Stopped(m) => m,
                                };
                                self.record(a, Some(key), "unknown", err_text(&format!("{}: the outcome is unknown ({}).", a.tool, clean(&m)), false), true, f.label, None).await
                            }
                        }
                    }
                    Gate::Stop(v) => Ok(v),
                }
            }
        }
    }

    /// The live definition, screened right before acting, so one changed
    /// after it was listed cannot run unscreened (spec rev 140). The error
    /// says whether trying again may work.
    pub(crate) async fn check_pin(&self, c: &Connector, f: &Facts, person: &str, run: &str) -> std::result::Result<(), (String, bool)> {
        let secret = self.connector_secret(c, person).await.ok().flatten();
        let live = self.app_tools(c, secret.as_ref().map(|s| s.expose())).await.map_err(|e| (format!("could not read {}'s tool definitions ({e})", f.app), true))?;
        let Some(t) = live.iter().find(|t| t.name == f.remote) else { return Err((format!("{} no longer offers {}", f.app, f.remote), true)) };
        if let Err(e) = self.pin(c, &f.name, t, Some(run).filter(|r| !r.is_empty())).await {
            return Err((format!("could not screen the tool definition: {e}"), true));
        }
        self.tool_refusal(&f.name).map_or(Ok(()), |why| Err((why, false)))
    }

    async fn lease(&self, a: &Action, f: &Facts) -> Result<Gate> {
        // The record it changes: its `id` or `record_id`, or the id field
        // the app's manifest names for what its sends act on (Gmail's
        // `draft_id`). A draft being sent is then changed by no other run
        // from the gate, which reads it, to the send (#177).
        let named = f.connector.iter().flat_map(|c| c.destinations.values().map(|d| d.id.as_str()));
        let id = ["id", "record_id"].into_iter().chain(named).find_map(|k| a.input.get(k).and_then(|v| v.as_str().map(String::from)));
        let Some(id) = id else { return Ok(Gate::Pass) };
        let resource = format!("{}:{id}", f.app);
        let holder = {
            let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
            match leases.get(&resource) {
                Some(h) if h != &a.run => Some(h.clone()),
                _ => {
                    leases.insert(resource.clone(), a.run.clone());
                    None
                }
            }
        };
        if let Some(h) = holder {
            let still = self.read(|s| s.runs.get(&h).is_some_and(|r| r.status == crate::state::Status::Running));
            if still {
                return Ok(Gate::Stop(self.deny(a, "lease", &format!("another task is changing {resource} right now"), true, None).await?));
            }
            self.leases.lock().unwrap_or_else(|e| e.into_inner()).insert(resource, a.run.clone());
        }
        Ok(Gate::Pass)
    }

    /// Ok(Ok(None)) allow; Ok(Ok(Some(why))) ask; Ok(Err(verdict)) denied.
    async fn policy_stage(
        &self,
        a: &Action,
        run: &Run,
        person: &Person,
        f: &Facts,
        shown: Option<&Shown>,
    ) -> Result<std::result::Result<Option<(String, Option<String>)>, Verdict>> {
        // The one rule of what this run may use, as offered (`run::admission`).
        match self.admits(run, &a.tool) {
            Ok(()) => {}
            Err(Refusal::Halted { reason, said }) => {
                self.decision(a, "deny", "policy", &said, None).await?;
                self.suspend(&a.run, &reason, None).await?;
                return Ok(Err(Verdict::Suspended));
            }
            Err(Refusal::Denied { why, rule }) => return Ok(Err(self.deny(a, "policy", &why, false, rule.map(Into::into)).await?)),
        }
        if f.risk == Risk::Prohibited {
            return Ok(Err(self.deny(a, "policy", &format!("{} is prohibited on this server", a.tool), false, None).await?));
        }
        if let Some(why) = self.hard_denial(a, run) {
            return Ok(Err(self.deny(a, "policy", &why, false, Some(HARD_DENIAL.into())).await?));
        }
        if !has_rights(person, f) {
            let why = format!("{} does not have access to {} (identity-provider groups)", person.id, f.app);
            return Ok(Err(self.deny(a, "policy", &why, false, None).await?));
        }
        if a.issuer == Issuer::Memory && f.name != "memory_write" {
            return Ok(Err(self.deny(a, "policy", "the memory pass may only write memory", false, None).await?));
        }
        // The person's own memory is not the world: writing it sends nothing (M1).
        let own_memory = f.connector.is_none() && f.app == "memory";
        if a.issuer.reads_only() && f.risk != Risk::Read {
            return Ok(Err(self.deny(a, "policy", "a read-back may only read", false, None).await?));
        }
        let (untrusted, private) = (run.untrusted, run.private);
        let policies = self.read(|s| s.org.policies().clone());
        let facts = ToolFacts::from(f);
        let outcome = policy::decide(&policies, person, Some(&run.task), &facts, untrusted, private)?;
        // A policy's ask is no longer an ask of its own (spec rev 140): Auto
        // Review judges every boundary action. Only its denials stand here.
        if let Outcome::Deny { reason, rule } = outcome {
            return Ok(Err(self.deny(a, "policy", &reason, false, rule).await?));
        }
        let mut ask = None;
        match self.local_gate(a, f, person) {
            Some(Err(why)) => return Ok(Err(self.deny(a, "policy", &why, false, None).await?)),
            Some(Ok(asked)) => ask = Some(asked),
            None => {}
        }
        // A loop run's tool calls need the act action too: the org can
        // keep autonomous acts off an app the person's own tasks may
        // still use (#368). Denied outright, never asked: there is no
        // one to ask mid-sweep.
        if let Some(why) = self.loop_deny(run, "act", Some(&facts)) {
            return Ok(Err(self.deny(a, "policy", &why, false, None).await?));
        }
        // Rule of Two and the audience rule are fixed in the kernel (K47).
        // A call carries data out if it changes state, fetches from the web, or
        // reaches an app whose audience is wider than the person (its inputs
        // travel there even when it only reads).
        // Starting a task from chat sends nothing out: the task carries the
        // turn's taint, and the rules apply to what it sends.
        // A tool that stays in the person's own account sends nothing out.
        // Nor does a read through the person's own sign-in: its query goes
        // only to the provider that holds their data already, and it reaches
        // no one there (#175). So does an identity's page, read at its own site.
        // A read through the organisation's key, the web and the browser keep it.
        // 4a. Compartment & web query guardrails (zero internet approval cards):
        // - `web_search` passes the shared query gate (scrub + budget + mandatory
        //   classifier when private && untrusted + retry cap), never prompting the user.
        // - `research_web` runs the same gate inside `compartment::research_web`.
        // - A link-following-only `OpenWeb` worker may only open URLs from its own
        //   search results, loaded pages, or the user's original prompt.
        // - A tainted `Private` run silently denies model-fabricated URLs instead
        //   of interrupting the user with an approval card.
        let wr = crate::tool_registry::web_read(&a.tool);
        if wr == crate::tool_registry::WebRead::Query && f.connector.is_none() {
            let q = a.input["query"].as_str().unwrap_or_default();
            if let Some(why) = self.check_web_query(a, run, q).await? {
                return Ok(Err(self.deny(a, "policy", &why, false, None).await?));
            }
        }
        let is_query_tool = matches!(wr, crate::tool_registry::WebRead::Query | crate::tool_registry::WebRead::Research);
        let web_or_browser_read = f.connector.is_none() && f.risk == Risk::Read && (f.app == "web" || (f.app == "browser" && !a.tool.starts_with("browser_as_")));
        let mut url_cleared = false;
        if web_or_browser_read && !is_query_tool && untrusted && private {
            if self.plain_web_read(a, f, run).await? {
                url_cleared = true;
            } else {
                let raw_url = a.input["url"].as_str().unwrap_or_default();
                // A constructed URL is never cleared here: it asks (case 3)
                // or is denied (decisions/2026-11-22-a-classifier-clear-is-not-a-pass.md).
                if let crate::compartment::query_gate::UrlVerdict::Deny(why) = self.check_web_url(a, run, f, raw_url).await? {
                    return Ok(Err(self.deny(a, "policy", &why, false, None).await?));
                }
            }
        }

        let own_account_read = f.risk == Risk::Read && f.connector.as_ref().is_some_and(|c| c.auth.as_deref() == Some(crate::connectors::OAUTH));
        let transmits = crate::local::may_send(a, f)
            && !own_memory
            && !f.reaches_self
            && !is_query_tool
            && (f.risk.changes_state()
                || f.app == "web"
                || (f.app == "browser" && !(f.risk == Risk::Read && a.tool.starts_with("browser_as_")))
                || (f.connector.is_some() && f.audience > Audience::Private && !own_account_read));
        // A web read whose address the model did not make up carries nothing
        // private out (decisions/2026-11-12-web-reads-by-provenance.md).
        let transmits = transmits && !(untrusted && private && url_cleared);
        // Outbound writes when holding untrusted content + private data:
        // check against the lazy user-only Envelope (never whole-directory or
        // blanket past-contact auto-allow). Out-of-envelope stageable writes
        // (and cross-thread replies to untrusted senders) are staged as drafts
        // instead of blocking on a modal approval card.
        let dests = self.destinations_of(a, f, shown);
        if transmits && untrusted && private {
            let env = self.ensure_envelope(run).await?;
            let typed: Vec<crate::envelope::Destination> = dests.iter().filter_map(|d| crate::envelope::Destination::parse_for_app(&f.app, d)).collect();
            let in_envelope = !typed.is_empty() && env.allows_all(&typed);
            let cross_thread_reply = in_envelope && crate::envelope::is_cross_thread_untrusted_reply(run, &f.app);
            if !in_envelope || cross_thread_reply {
                let why = if cross_thread_reply {
                    "cross-thread reply to an untrusted sender while holding private data from outside this thread"
                } else {
                    "destination is outside the user's request envelope"
                };
                if let Some(kind) = crate::staging::classify_stageable(&f.app, &a.tool, f.risk, &a.input) {
                    let msg = self.stage_action(a, run, f, shown, kind, &dests, why).await?;
                    self.decision(a, "stage", "envelope", why, None).await?;
                    return Ok(Err(self.record(a, None, "staged", msg, false, Label::PRINCIPAL, None).await?));
                }
                ask = Some((
                    format!("the work has read untrusted content and private data, and this unstageable action targets a new destination ({why})"),
                    Some(crate::asks::NEW_DESTINATION.to_string()),
                ));
            }
        }
        Ok(Ok(self.payment_ask(a, f.risk).or(ask)))
    }

    /// Resolve an intent that has no result (B7): resend when the app honors
    /// idempotency keys or the call cannot have external effect; otherwise mark
    /// it unknown and ask the person.
    async fn reconcile(&self, a: &Action, f: &Facts, key: &str) -> Result<Gate> {
        if f.app == "sandbox" {
            let text = err_text(&format!("{} was interrupted by a restart; its outcome is unknown. Check the workspace before doing it again.", a.tool), false);
            return Ok(Gate::Stop(self.record(a, Some(key.into()), "unknown", text, true, f.label, None).await?));
        }
        // A chat call's ids come from its key, so resending finds what the
        // first call made.
        if f.risk == Risk::Read || is_chat_tool(f) {
            return Ok(Gate::Pass);
        }
        if f.connector.as_ref().is_some_and(|c| c.idempotency == "header") {
            self.decision(a, "allow", "idempotency", "resending with the same idempotency key", None).await?;
            return Ok(Gate::Pass);
        }
        // Not safe to resend, and a retry never asks (spec rev 140).
        self.decision(a, "deny", "idempotency", "outcome unknown and the app cannot deduplicate", None).await?;
        let text = err_text(&crate::asks::outcome_unknown(self, a, f).await?, false);
        Ok(Gate::Stop(self.record(a, Some(key.into()), "unknown", text, true, f.label, None).await?))
    }
}

#[cfg(test)]
#[path = "boundary_transport_tests.rs"]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::clamp_output_for_budget;

    /// $10 a million output tokens, the 16000-token allowance: the full
    /// allowance when it fits, fewer when only fewer fit, none when the
    /// prompt alone does not (#273).
    #[test]
    fn the_dollars_left_bound_the_call_output() {
        // $1 left, $0.01 in, $0.16 out: the full 16000.
        assert_eq!(clamp_output_for_budget(1.0, 0.01, 0.16, 16000), Some(16000));
        // $0.050001 left, $0.01 in: 4000 fit.
        assert_eq!(clamp_output_for_budget(0.050001, 0.01, 0.16, 16000), Some(4000));
        // $0.010009 left: nothing writable fits.
        assert_eq!(clamp_output_for_budget(0.010009, 0.01, 0.16, 16000), None);
        // The prompt alone past what is left: no call.
        assert_eq!(clamp_output_for_budget(0.001, 0.01, 0.16, 16000), None);
        // Free output (or unpriced): the full allowance while the prompt fits.
        assert_eq!(clamp_output_for_budget(0.05, 0.01, 0.0, 16000), Some(16000));
    }
}
