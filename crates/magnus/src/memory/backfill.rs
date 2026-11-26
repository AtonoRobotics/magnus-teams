//! Idempotent projection of existing Magnus memory/knowledge/playbook records
//! into the unified Postgres memory engine for `Shadow` and `V2` modes.

use crate::memory::enrich::{CommitOutcome, EnrichmentPayload};
use crate::memory::{AssertionInput, MemoryEngine, SourceInput, TrustLabel, DEFAULT_TENANT_ID};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;

/// A legacy personal memory, company KB entry, playbook lesson, or team entry
/// ready to project into `memory_sources` + `memory_assertions`.
#[derive(Clone, Debug)]
pub struct LegacyRecord {
    pub id: String,
    pub kind: String,
    pub owner_id: Option<String>,
    pub readers: Vec<Vec<String>>,
    pub label: TrustLabel,
    pub text: String,
    pub occurred_at: DateTime<Utc>,
    pub embedding: Option<Vec<f32>>,
    pub model: Option<String>,
}

/// Summary of a backfill pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackfillReport {
    pub ingested: u64,
    pub deduplicated: u64,
    pub assertions_projected: u64,
}

impl MemoryEngine {
    /// Idempotently project legacy records into `memory_sources` and
    /// `memory_assertions` with full `memory_evidence` lineage.
    pub async fn backfill_legacy_records(&self, records: &[LegacyRecord]) -> Result<BackfillReport> {
        let mut report = BackfillReport::default();
        for rec in records {
            let source_id = format!("legacy_{}", rec.id);
            let ingest = self
                .ingest_source(&SourceInput {
                    tenant_id: DEFAULT_TENANT_ID.into(),
                    source_id: source_id.clone(),
                    rev: 1,
                    owner_id: rec.owner_id.clone(),
                    readers: rec.readers.clone(),
                    label: rec.label,
                    kind: rec.kind.clone(),
                    text: rec.text.clone(),
                    metadata: json!({ "legacy_id": rec.id }),
                    occurred_at: rec.occurred_at,
                })
                .await?;

            if ingest.deduplicated {
                report.deduplicated += 1;
                continue;
            }
            report.ingested += 1;

            let payload = EnrichmentPayload {
                source_embedding: rec.embedding.clone(),
                model: rec.model.clone(),
                assertions: vec![AssertionInput {
                    assertion_id: rec.id.clone(),
                    kind: rec.kind.clone(),
                    statement: rec.text.clone(),
                    valid_from: Some(rec.occurred_at),
                    valid_to: None,
                    confidence: 1.0,
                    embedding: rec.embedding.clone(),
                    model: rec.model.clone(),
                }],
                entities: Vec::new(),
                edges: Vec::new(),
            };
            if self.commit_enrichment(DEFAULT_TENANT_ID, &source_id, 1, ingest.job_id, &payload).await? == CommitOutcome::Committed {
                report.assertions_projected += 1;
            }
        }
        Ok(report)
    }
}
