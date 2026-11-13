//! Reserve a priced call before contacting a provider. Reservations survive
//! cancellation and restart; an unknown bill retains its worst-case amount.
use super::{month, Change};
use crate::adapters::{AdapterError, TurnOut};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::State;
use magnus_adapter_proto::{ErrorClass, Message, ToolSpec};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Charge {
    pub id: String,
    pub run: String,
    pub person: String,
    pub workspace: Option<String>,
    pub channel: String,
    pub model: String,
    pub month: String,
    pub usd: f64,
    pub settled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin_app::tests::server_with;
    use axum::Router;

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_reviews_reserve_the_same_monthly_allowance_once() {
        let s = server_with("", |_| Router::new()).await;
        s.k.emit("test", Ev::Spend(Change::LimitSet { scope: super::super::Scope::Server, usd: Some(1.0) })).await.unwrap();
        let (a, b) = tokio::join!(s.k.reserve_model_call("", "sam", "a", "m", 0.6, 0.2, 100), s.k.reserve_model_call("", "mo", "a", "m", 0.6, 0.2, 100));
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        let charge = a.or(b).unwrap().0;
        let before = s.k.read(|s| month::spent(s, &charge.month).total_usd);
        assert_eq!(before, 0.8, "an in-flight or unknown charge was not counted");
        // Replaying the reservation keeps it charged, including an interrupted review.
        let mut replay = State::default();
        replay.spend.apply(chrono::Utc::now(), &Change::CallReserved { charge: charge.clone(), baseline: None });
        assert_eq!(month::spent(&replay, &charge.month).total_usd, before);
        s.k.emit("test", Ev::Spend(Change::CallSettled { id: charge.id, usd: 0.1 })).await.unwrap();
        assert_eq!(s.k.read(|s| month::spent(s, &charge.month).total_usd), 0.1);
        assert!(s.k.reserve_model_call("", "mo", "a", "m", 0.6, 0.2, 100).await.is_ok());
        s.done().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_reviews_stop_at_the_workspace_limit() {
        let s = server_with("", |_| Router::new()).await;
        s.k.emit("test", Ev::Team(crate::team::Change::Created { workspace: "w".into(), name: "W".into(), owner: "sam".into() })).await.unwrap();
        s.k.emit("test", Ev::Team(crate::team::Change::Chat { workspace: "w".into(), chat: crate::team::chat::Chat::SpendLimit { channel: None, usd: Some(0.5) } })).await.unwrap();
        assert!(s.k.reserve_model_call("", "workspace:w", "a", "m", 0.6, 0.0, 100).await.is_err());
        assert!(s.k.read(|s| s.spend.calls.is_empty()));
        assert!(s.k.reserve_model_call("", "workspace:w", "a", "m", 0.1, 0.2, 100).await.is_ok());
        assert!(s.k.reserve_model_call("", "workspace:w", "a", "m", 0.3, 0.0, 100).await.is_err());
        assert_eq!(s.k.read(|s| crate::team::spend::spent_this_month(s, "w", chrono::Utc::now()).total_usd), 0.1 + 0.2);
        s.done().await;
    }
}

pub fn legacy(s: &State, run: &crate::state::Run) -> f64 {
    s.spend.call_baselines.get(&run.id).copied().unwrap_or(run.usd)
}

fn task_spent(s: &State, task: &str) -> f64 {
    let runs: Vec<_> = s.runs.values().filter(|r| s.lineage(&r.id).iter().any(|t| t == task)).collect();
    runs.iter().map(|r| legacy(s, r)).sum::<f64>() + s.spend.calls.values().filter(|c| runs.iter().any(|r| r.id == c.run)).map(|c| c.usd).sum::<f64>()
}

fn remaining(s: &State, run: &str, person: &str, workspace: Option<&str>, channel: &str) -> anyhow::Result<f64> {
    let mut left = super::enforce::left_now(s, person, chrono::Utc::now())?.unwrap_or(f64::INFINITY);
    for task in s.lineage(run) {
        if let Some(r) = s.tasks.get(&task).and_then(|t| s.runs.get(&t.run_id)) {
            left = left.min(r.config.budget.max_usd - task_spent(s, &task));
        }
    }
    if let Some(ws) = workspace {
        let w = s.team.workspace(ws).ok_or_else(|| anyhow::anyhow!("workspace no longer exists"))?;
        let spent = crate::team::spend::spent_this_month(s, ws, chrono::Utc::now());
        left = left.min(crate::team::spend::left_usd(w, &spent, channel).unwrap_or(f64::INFINITY));
    }
    Ok(left.max(0.0))
}

fn refused(message: impl ToString) -> AdapterError {
    AdapterError { class: ErrorClass::InvalidRequest, message: message.to_string(), retry_after_s: None, usage: None }
}

impl Kernel {
    /// All paid model paths use this boundary, including reviews without a task.
    /// The caller still records its normal reply; spend uses the authoritative
    /// call ledger after the first reservation, without counting that reply twice.
    pub(crate) async fn paid_complete(
        &self,
        run: &str,
        principal: &str,
        adapter: &str,
        model: &str,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<ToolSpec>,
        max_tokens: u32,
        on_text: impl FnMut(&str),
    ) -> Result<TurnOut, AdapterError> {
        let input = self.adapters.estimate_prompt(adapter, model, system, messages.clone(), tools.clone()).await.map_err(refused)?;
        let (input_usd, output_usd) = self.adapters.quote(adapter, model, input, max_tokens).await.map_err(refused)?;
        let (charge, tokens) = self.reserve_model_call(run, principal, adapter, model, input_usd, output_usd, max_tokens).await?;
        let result = self.adapters.complete(adapter, model, system, messages, tools, tokens, on_text).await;
        let usage = match &result {
            Ok(out) => Some(&out.usage),
            Err(e) => e.usage.as_ref(),
        };
        if let Some(usage) = usage {
            if !usage.cost_usd.is_finite() || usage.cost_usd < 0.0 {
                return Err(refused("the provider returned invalid usage; reservation retained"));
            }
            let _guard = self.paid_calls.lock().await;
            self.emit("kernel:spend", Ev::Spend(Change::CallSettled { id: charge.id, usd: usage.cost_usd })).await.map_err(refused)?;
        }
        result
    }
    async fn reserve_model_call(
        &self,
        run: &str,
        principal: &str,
        adapter: &str,
        model: &str,
        input_usd: f64,
        output_usd: f64,
        max_tokens: u32,
    ) -> Result<(Charge, u32), AdapterError> {
        if !input_usd.is_finite() || !output_usd.is_finite() || input_usd < 0.0 || output_usd < 0.0 {
            return Err(refused("the provider returned an invalid price"));
        }
        let reserved = {
            let _guard = self.paid_calls.lock().await;
            let (person, workspace, channel, baseline, left) = self.read(|s| {
                let r = s.runs.get(run);
                let person = r.map_or(principal, |r| r.person.as_str()).to_string();
                let team = s.authority(run).and_then(|t| s.team.run_of(&t.task_id));
                let workspace = team.map(|t| t.workspace.clone()).or_else(|| principal.strip_prefix("workspace:").map(String::from));
                let channel = team.map(|t| t.channel.clone()).unwrap_or_default();
                let baseline = r.filter(|r| !s.spend.call_baselines.contains_key(&r.id)).map(|r| r.usd);
                let left = remaining(s, run, &person, workspace.as_deref(), &channel);
                (person, workspace, channel, baseline, left)
            });
            let left = left.map_err(refused)?;
            let tokens = crate::boundary::clamp_output_for_budget(left, input_usd, output_usd, max_tokens)
                .ok_or_else(|| refused("the next model call exceeds the remaining spend allowance"))?;
            let usd = input_usd + output_usd * f64::from(tokens) / f64::from(max_tokens.max(1));
            let charge = Charge {
                id: new_id("call"),
                run: run.into(),
                person,
                workspace,
                channel,
                model: format!("{adapter}/{model}"),
                month: month::key(chrono::Utc::now()),
                usd,
                settled: false,
            };
            self.emit("kernel:spend", Ev::Spend(Change::CallReserved { charge: charge.clone(), baseline })).await.map_err(refused)?;
            (charge, tokens)
        };

        Ok(reserved)
    }
}
