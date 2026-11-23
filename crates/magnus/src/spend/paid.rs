//! One way to make a paid model call (decisions/2026-11-23-one-way-to-make-a-paid-call.md).
//!
//! Every model call outside the agent loop's own step goes through
//! [`Kernel::paid_call`]: the reviewer and the checks that share its model,
//! the memory pass, knowledge extraction, the spoken script, the safety
//! monitor, the screens and the credential review. It does four things,
//! in order:
//!
//! 1. the model may be used: its provider is not turned off, and the
//!    person it works for may use it (a model the server set for its own
//!    checks is the server's choice, not theirs);
//! 2. what it may cost fits: its worst case, at the adapter's quote, fits
//!    what the run has left under every limit (`allowance`), or what its
//!    owner has left when there is no run (a person's month, a workspace's,
//!    the server's), less what calls in flight hold (`holds`);
//! 3. the call;
//! 4. its cost is recorded, on success and on failure alike: a failed call
//!    the provider charged for is charged too. A run's call is recorded as
//!    its caller's own usage event, or as a `charged` change; a call for no
//!    run is a `charged` change naming its owner. Each lands on the money
//!    ledger (`book`).
//!
//! The agent loop's own step records its `model_reply` and `model_error`
//! itself and takes its hold through the same allowance; it asks
//! [`provider_on`] before each attempt, as this does.

use super::allowance::{allowance_held, owner_allowance, scope_of_run, Owner, Spender};
use super::Change;
use crate::adapters::TurnOut;
use crate::boundary::estimated_input_tokens;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Run;
use chrono::Utc;
use magnus_adapter_proto::{Message, ToolSpec};
use std::time::Duration;

/// Who writes a `charged` change.
const PAID_ACTOR: &str = "kernel:paid";

/// Who a paid call is for.
pub(crate) enum Payer<'a> {
    /// A run's call, as `Spender::Extra` (an optional extra) or
    /// `Spender::Check` (a safety check on its work).
    Run(&'a Run, Spender),
    /// A call made for no run.
    Owner(Owner),
}

/// One paid model call.
pub(crate) struct Call<'a> {
    pub payer: Payer<'a>,
    /// What it is for, as its record names it.
    pub purpose: &'a str,
    pub adapter: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    /// Its output allowance: the call runs with all of it or not at all,
    /// as a truncated judgment is worse than none.
    pub max_tokens: u32,
    /// The model is one the server set for its own checks (`[safety]`, the
    /// default for a credential review): a person's model access does not
    /// limit it. Its provider being off still does.
    pub servers_model: bool,
    /// How long the provider has to answer.
    pub timeout: Option<Duration>,
}

/// Why a paid call gave no answer.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The model may not be used.
    Model(String),
    /// What is left does not cover its worst case.
    Allowance(String),
    /// The provider failed or did not answer in time; what it charged is
    /// recorded.
    Failed(String),
    /// Its record could not be written.
    Record(anyhow::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Model(why) => write!(f, "the model may not be used: {why}"),
            Refused::Allowance(why) => write!(f, "nothing left to spend on it: {why}"),
            Refused::Failed(why) => write!(f, "the model provider failed: {why}"),
            Refused::Record(e) => write!(f, "its record could not be written: {e:#}"),
        }
    }
}

impl std::error::Error for Refused {}

/// A paid call's answer, and the seq of the event that recorded it.
pub(crate) struct Paid {
    pub out: TurnOut,
    pub seq: i64,
}

/// Why `adapter` may not be used now: an admin turned its provider off.
pub fn provider_on(k: &Kernel, adapter: &str) -> Result<(), String> {
    match k.read(|s| s.org.providers_off().contains(adapter)) {
        true => Err(format!("the provider {adapter} is turned off on this server")),
        false => Ok(()),
    }
}

/// Why the person may not use `adapter/model`, when they may not.
async fn person_may_not(k: &Kernel, person: &str, adapter: &str, model: &str) -> Option<String> {
    let allowed = k.allowed_models(person).await;
    let list = || allowed.iter().flatten().cloned().collect::<Vec<_>>();
    (!super::access::permits(&allowed, adapter, model)).then(|| super::enforce::ModelNotAllowed { model: format!("{adapter}/{model}"), allowed: list(), off: false }.to_string())
}

impl Kernel {
    /// The next of the server's fallback models the person may use and
    /// whose provider is on, after `idx`, other than the one `active`.
    pub(crate) async fn next_fallback(&self, person: &str, active: (&str, &str), idx: &mut usize) -> Option<(String, String)> {
        let allowed = self.allowed_models(person).await;
        let off = self.read(|s| s.org.providers_off().clone());
        while let Some(entry) = self.cfg.model.fallbacks.get(*idx) {
            *idx += 1;
            let Some((a, m)) = entry.split_once('/').map(|(a, m)| (a.trim(), m.trim())).filter(|(a, m)| !a.is_empty() && !m.is_empty()) else { continue };
            if (a, m) != active && super::access::usable(&allowed, &off, a, m) {
                return Some((a.to_string(), m.to_string()));
            }
        }
        None
    }

    /// Make one paid model call: allowed, within what is left, made, and
    /// recorded whether it answers or fails. `record` gives a run's call its
    /// usage event and the actor that writes it; None, or a call for no
    /// run, records a `charged` change.
    pub(crate) async fn paid_call(&self, c: Call<'_>, record: impl FnOnce(&TurnOut) -> Option<(&'static str, Ev)>) -> Result<Paid, Refused> {
        provider_on(self, c.adapter).map_err(Refused::Model)?;
        let person = match &c.payer {
            Payer::Run(r, _) => Some(r.person.clone()),
            Payer::Owner(Owner::Person(p)) => Some(p.clone()),
            Payer::Owner(_) => None,
        };
        if let Some(why) = match (&person, c.servers_model) {
            (Some(p), false) => person_may_not(self, p, c.adapter, c.model).await,
            _ => None,
        } {
            return Err(Refused::Model(why));
        }
        let worst = self.worst_case(&c).await;
        let scope = match &c.payer {
            Payer::Run(r, _) => self.read(|s| scope_of_run(s, &r.id)),
            Payer::Owner(o) => o.scope(),
        };
        let fit = |held: &[super::holds::Hold]| {
            let left = self.read(|s| match &c.payer {
                Payer::Run(r, spender) => allowance_held(s, &r.id, *spender, Utc::now(), held),
                Payer::Owner(o) => owner_allowance(s, o, Utc::now(), held),
            });
            match (left, worst) {
                (Err(stop), _) => Err(stop.why),
                (Ok(l), Some(w)) if w <= l.usd => Ok(((), w)),
                (Ok(l), Some(w)) => Err(format!("its worst case (${w:.4}) does not fit what is left of {} (${:.4})", l.named(), l.usd)),
                // A call nobody can price passes no limit unseen: only
                // where no limit applies does it go on.
                (Ok(l), None) if l.usd.is_infinite() => Ok(((), 0.0)),
                (Ok(_), None) => Err("it could not be priced".to_string()),
            }
        };
        let ((), held) = self.spend_holds.admit(scope, fit).await.map_err(|why| {
            tracing::info!("{} skipped: {why}", c.purpose);
            Refused::Allowance(why)
        })?;
        let model = format!("{}/{}", c.adapter, c.model);
        let call = self.adapters.complete(c.adapter, c.model, c.system, c.messages, c.tools, c.max_tokens, |_| {});
        let out = match c.timeout {
            Some(t) => tokio::time::timeout(t, call).await.map_err(|_| Refused::Failed(format!("no answer within {}s", t.as_secs())))?,
            None => call.await,
        };
        let (run, owner) = match &c.payer {
            Payer::Run(r, _) => (Some(r.id.clone()), None),
            Payer::Owner(o) => (None, Some(o.key())),
        };
        let charged = |usage, failed| Ev::Spend(Change::Charged { run: run.clone(), owner: owner.clone(), purpose: c.purpose.to_string(), model: model.clone(), usage, failed });
        let result = match out {
            Ok(out) => {
                let (actor, ev) = run.as_ref().and_then(|_| record(&out)).unwrap_or_else(|| (PAID_ACTOR, charged(out.usage.clone(), false)));
                let e = self.emit(actor, ev).await.map_err(Refused::Record)?;
                Ok(Paid { out, seq: e.seq })
            }
            Err(e) => {
                tracing::warn!("{} on {model}: {e:?}", c.purpose);
                if let Some(usage) = e.usage.clone() {
                    self.emit(PAID_ACTOR, charged(usage, true)).await.map_err(Refused::Record)?;
                }
                Err(Refused::Failed(e.message))
            }
        };
        drop(held);
        result
    }

    /// A call's worst case in dollars: its prompt and its whole output at
    /// the adapter's quote. None when the adapter cannot quote.
    async fn worst_case(&self, c: &Call<'_>) -> Option<f64> {
        let input_tokens = match self.adapters.estimate_prompt(c.adapter, c.model, c.system, c.messages.clone(), c.tools.clone()).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("{} prompt estimate: {e:#}; pricing the kernel's bytes", c.purpose);
                estimated_input_tokens(c.system, &c.messages, &c.tools)
            }
        };
        match self.adapters.quote(c.adapter, c.model, input_tokens, c.max_tokens).await {
            Ok((input_usd, output_usd)) => Some(input_usd + output_usd),
            Err(e) => {
                tracing::warn!("{} quote: {e:#}", c.purpose);
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "paid_tests.rs"]
mod tests;
