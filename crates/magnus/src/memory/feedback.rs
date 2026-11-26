//! Outcome feedback weighting (EMA) and access frequency tracking.
//!
//! Zero-LLM improvement stage: nudges `feedback_weight` up when recalled
//! assertions lead to accepted outcomes and sharply down when corrected.
//! Enforces the taint guardrail: `untrusted` runs may never boost clean assertions.

use crate::memory::{MemoryEngine, FEEDBACK_EMA_ALPHA, MAX_FEEDBACK_WEIGHT, MIN_FEEDBACK_WEIGHT};
use anyhow::{Context, Result};

/// Outcome signal observed after a run that recalled one or more assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackSignal {
    Accepted,
    Edited,
    Corrected,
}

impl FeedbackSignal {
    pub fn target_weight(self) -> f64 {
        match self {
            Self::Accepted => 1.5,
            Self::Edited => 0.85,
            Self::Corrected => 0.25,
        }
    }

    pub fn is_positive(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

/// Pure EMA step clamped to `[MIN_FEEDBACK_WEIGHT, MAX_FEEDBACK_WEIGHT]`.
pub fn next_feedback_weight(current: f64, signal: FeedbackSignal) -> f64 {
    let raw = (1.0 - FEEDBACK_EMA_ALPHA) * current + FEEDBACK_EMA_ALPHA * signal.target_weight();
    raw.clamp(MIN_FEEDBACK_WEIGHT, MAX_FEEDBACK_WEIGHT)
}

impl MemoryEngine {
    /// Apply an EMA feedback update to recalled assertions.
    ///
    /// Guardrail: if `run_untrusted` is true and the signal is positive, clean
    /// (`untrusted = false`) assertions are skipped so untrusted runs cannot
    /// amplify clean memory weights.
    pub async fn record_feedback(&self, tenant_id: &str, assertion_ids: &[String], signal: FeedbackSignal, run_untrusted: bool) -> Result<u64> {
        if assertion_ids.is_empty() {
            return Ok(0);
        }
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let target = signal.target_weight();
        let skip_clean = run_untrusted && signal.is_positive();

        let updated = client
            .execute(
                "UPDATE public.memory_assertions
                 SET feedback_weight = LEAST(
                         $6::double precision,
                         GREATEST(
                             $5::double precision,
                             (1.0 - $3::double precision) * feedback_weight
                             + $3::double precision * $4::double precision
                         )
                     ),
                     updated_at = now()
                 WHERE tenant_id = $1
                   AND assertion_id = ANY($2)
                   AND (NOT $7::boolean OR untrusted = true)",
                &[&tenant_id, &assertion_ids, &FEEDBACK_EMA_ALPHA, &target, &MIN_FEEDBACK_WEIGHT, &MAX_FEEDBACK_WEIGHT, &skip_clean],
            )
            .await
            .context("update memory_assertions feedback_weight")?;
        Ok(updated)
    }

    /// Increment `access_count` and stamp `last_accessed_at` for recalled assertions.
    pub async fn bump_access_counts(&self, tenant_id: &str, assertion_ids: &[String]) -> Result<u64> {
        if assertion_ids.is_empty() {
            return Ok(0);
        }
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let updated = client
            .execute(
                "UPDATE public.memory_assertions
                 SET access_count = access_count + 1,
                     last_accessed_at = now()
                 WHERE tenant_id = $1 AND assertion_id = ANY($2)",
                &[&tenant_id, &assertion_ids],
            )
            .await
            .context("bump memory_assertions access_count")?;
        Ok(updated)
    }
}
