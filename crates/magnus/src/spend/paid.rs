//! One way to make a paid model call (decisions/2026-11-23-one-way-to-make-a-paid-call.md).
//!
//! Every model call outside the agent loop's own step goes through
//! [`Kernel::paid_call`]: the reviewer and the checks that share its model,
//! the memory pass, knowledge extraction, the spoken script, the safety
//! monitor, the screens and the credential review. Pictures and knowledge
//! embeddings go through the same steps by [`Kernel::pay`], priced by the
//! server's config (decisions/2026-11-23-every-charged-token-is-booked.md).
//! It does four things, in order:
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
//!    the provider charged for is charged too, and one cut off by its
//!    timeout is charged its prompt at the quoted input price, as the
//!    provider bills a prompt it was sent. A run's call is recorded as
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
use magnus_adapter_proto::{Message, ToolSpec, Usage};
use std::time::Duration;

/// Who writes a `charged` change.
const PAID_ACTOR: &str = "kernel:paid";

/// A call's prompt, and what is quoted for it and for its whole output:
/// its worst case is the two together.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Quote {
    pub input_tokens: u64,
    pub input_usd: f64,
    pub output_usd: f64,
}

/// Any paid call, whatever it makes (`Kernel::pay`).
pub(crate) struct Bill<'a> {
    pub payer: Payer<'a>,
    /// What it is for, as its record names it.
    pub purpose: &'a str,
    /// The adapter that makes it; for a provider that is no adapter (an
    /// images or embeddings endpoint), what it is. Its record names it.
    pub adapter: &'a str,
    pub model: &'a str,
    /// The server set this model for its own use (`Call::servers_model`).
    pub servers_model: bool,
    /// How long the provider has to answer (`Call::timeout`).
    pub timeout: Option<Duration>,
}

/// Why a paid call failed, and what the provider charged for it by then.
pub(crate) struct Failure {
    pub message: String,
    pub usage: Option<Usage>,
}

pub(crate) fn no_usage() -> Usage {
    Usage { input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: 0.0 }
}

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
    /// How long the provider has to answer. The call owns it: one cut off
    /// is booked here, which a timeout around the call could not do.
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

/// The next of `fallbacks` (`adapter/model`) after `idx` that may be used
/// (`access::usable`) and is not `active`; `idx` moves past it.
pub(crate) fn fallback_after(
    fallbacks: &[String],
    allowed: &Option<std::collections::BTreeSet<String>>,
    off: &std::collections::BTreeSet<String>,
    active: (&str, &str),
    idx: &mut usize,
) -> Option<(String, String)> {
    while let Some(entry) = fallbacks.get(*idx) {
        *idx += 1;
        let Some((a, m)) = entry.split_once('/').map(|(a, m)| (a.trim(), m.trim())).filter(|(a, m)| !a.is_empty() && !m.is_empty()) else { continue };
        if (a, m) != active && super::access::usable(allowed, off, a, m) {
            return Some((a.to_string(), m.to_string()));
        }
    }
    None
}

impl Kernel {
    /// The next of the server's fallback models the person may use and
    /// whose provider is on, after `idx`, other than the one `active`.
    pub(crate) async fn next_fallback(&self, person: &str, active: (&str, &str), idx: &mut usize) -> Option<(String, String)> {
        let allowed = self.allowed_models(person).await;
        let off = self.read(|s| s.org.providers_off().clone());
        fallback_after(&self.cfg.model.fallbacks, &allowed, &off, active, idx)
    }

    /// Make one paid model call: allowed, within what is left, made, and
    /// recorded whether it answers or fails. `record` gives a run's call its
    /// usage event and the actor that writes it; None, or a call for no
    /// run, records a `charged` change.
    pub(crate) async fn paid_call(&self, c: Call<'_>, record: impl FnOnce(&TurnOut) -> Option<(&'static str, Ev)>) -> Result<Paid, Refused> {
        let quote = self.quote(&c).await;
        let Call { payer, purpose, adapter, model, system, messages, tools, max_tokens, servers_model, timeout } = c;
        let bill = Bill { payer, purpose, adapter, model, servers_model, timeout };
        let call = async move {
            let out = self.adapters.complete(adapter, model, system, messages, tools, max_tokens, |_| {}).await;
            out.map(|out| {
                let usage = out.usage.clone();
                (out, usage)
            })
            .map_err(|e| Failure { message: e.message.clone(), usage: e.usage.clone() })
        };
        let (out, seq) = self.pay(bill, quote, call, |out, _| record(out)).await?;
        Ok(Paid { out, seq })
    }

    /// Why a call may not be made on this model, if so: its provider is
    /// off, or the person it works for may not use it.
    async fn model_refusal(&self, b: &Bill<'_>) -> Result<(), Refused> {
        provider_on(self, b.adapter).map_err(Refused::Model)?;
        let person = match &b.payer {
            Payer::Run(r, _) => Some(r.person.clone()),
            Payer::Owner(Owner::Person(p)) => Some(p.clone()),
            Payer::Owner(_) => None,
        };
        match (&person, b.servers_model) {
            (Some(p), false) => person_may_not(self, p, b.adapter, b.model).await.map_or(Ok(()), |why| Err(Refused::Model(why))),
            _ => Ok(()),
        }
    }

    /// Whether a call whose worst case is `worst` fits what its payer has
    /// left given what calls in flight hold, and the dollars it holds.
    fn fit(&self, payer: &Payer<'_>, worst: Option<f64>, held: &[super::holds::Hold]) -> Result<((), f64), String> {
        let left = self.read(|s| match payer {
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
    }

    /// Why a call could not be made now even with nothing else in flight,
    /// if so: for a caller that answers before the call is made (a
    /// picture), so a call refused is said at once. `pay` decides.
    pub(crate) async fn may_pay(&self, b: &Bill<'_>, quote: Option<Quote>) -> Result<(), Refused> {
        self.model_refusal(b).await?;
        self.fit(&b.payer, quote.map(|q| q.input_usd + q.output_usd), &[]).map(|_| ()).map_err(Refused::Allowance)
    }

    /// Make one paid call of any kind (`paid_call` for a model's turn): the
    /// model may be used, its worst case at `quote` fits what is left, the
    /// call is made, and its cost recorded whether it answers or fails. A
    /// call its timeout cuts off is booked at its quoted input price.
    pub(crate) async fn pay<T>(
        &self,
        b: Bill<'_>,
        quote: Option<Quote>,
        call: impl std::future::Future<Output = Result<(T, Usage), Failure>>,
        record: impl FnOnce(&T, &Usage) -> Option<(&'static str, Ev)>,
    ) -> Result<(T, i64), Refused> {
        self.model_refusal(&b).await?;
        let worst = quote.map(|q| q.input_usd + q.output_usd);
        let scope = match &b.payer {
            Payer::Run(r, _) => self.read(|s| scope_of_run(s, &r.id)),
            Payer::Owner(o) => o.scope(),
        };
        let ((), held) = self.spend_holds.admit(scope, |held| self.fit(&b.payer, worst, held)).await.map_err(|why| {
            tracing::info!("{} skipped: {why}", b.purpose);
            Refused::Allowance(why)
        })?;
        let model = format!("{}/{}", b.adapter, b.model);
        let (run, owner) = match &b.payer {
            Payer::Run(r, _) => (Some(r.id.clone()), None),
            Payer::Owner(o) => (None, Some(o.key())),
        };
        let charged = |usage, failed| Ev::Spend(Change::Charged { run: run.clone(), owner: owner.clone(), purpose: b.purpose.to_string(), model: model.clone(), usage, failed });
        let out = match b.timeout {
            Some(t) => match tokio::time::timeout(t, call).await {
                Ok(out) => out,
                Err(_) => {
                    // Cut off: the provider was sent the request and bills
                    // it, so it is booked at the quoted input price.
                    let why = format!("no answer within {}s", t.as_secs());
                    tracing::warn!("{} on {model}: {why}", b.purpose);
                    let usage = quote.map_or_else(no_usage, |q| Usage { input_tokens: q.input_tokens, cost_usd: q.input_usd, ..no_usage() });
                    self.emit(PAID_ACTOR, charged(usage, true)).await.map_err(Refused::Record)?;
                    return Err(Refused::Failed(why));
                }
            },
            None => call.await,
        };
        let result = match out {
            Ok((out, usage)) => {
                let (actor, ev) = run.as_ref().and_then(|_| record(&out, &usage)).unwrap_or_else(|| (PAID_ACTOR, charged(usage, false)));
                let e = self.emit(actor, ev).await.map_err(Refused::Record)?;
                Ok((out, e.seq))
            }
            Err(e) => {
                tracing::warn!("{} on {model}: {}", b.purpose, e.message);
                if let Some(usage) = e.usage {
                    self.emit(PAID_ACTOR, charged(usage, true)).await.map_err(Refused::Record)?;
                }
                Err(Refused::Failed(e.message))
            }
        };
        drop(held);
        result
    }

    /// A call's prompt and the adapter's quote for it and its whole output:
    /// its worst case. None when the adapter cannot quote.
    async fn quote(&self, c: &Call<'_>) -> Option<Quote> {
        let input_tokens = match self.adapters.estimate_prompt(c.adapter, c.model, c.system, c.messages.clone(), c.tools.clone()).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("{} prompt estimate: {e:#}; pricing the kernel's bytes", c.purpose);
                estimated_input_tokens(c.system, &c.messages, &c.tools)
            }
        };
        match self.adapters.quote(c.adapter, c.model, input_tokens, c.max_tokens).await {
            Ok((input_usd, output_usd)) => Some(Quote { input_tokens, input_usd, output_usd }),
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
