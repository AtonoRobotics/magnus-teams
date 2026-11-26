//! Unified Rust + Postgres memory engine (`decisions/2026-11-26-unified-postgres-memory.md`).
//!
//! Persists immutable versioned evidence (`memory_sources`) separately from derived
//! assertions (`memory_assertions`), canonical entities (`memory_entities`), typed
//! relationships (`memory_edges`), and lineage (`memory_evidence`). Enforces forced
//! Postgres RLS on every table, resolves supersession per-reader at query time, fences
//! async enrichment against live source revisions, and runs a 5-job gated curator.

pub mod backfill;
pub mod curator;
pub mod enrich;
pub mod erasure;
pub mod feedback;
pub mod graph;
pub mod ingest;
pub mod retrieve;
pub mod schema;
pub mod worker;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_postgres::Client;

/// Default tenant identifier for single-org deployments.
pub const DEFAULT_TENANT_ID: &str = "default";
/// RRF smoothing constant (`k = 60` standard).
pub const RRF_K: f64 = 60.0;
/// Half-life in days for recency decay.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 30.0;
/// Trust weight multiplier for untrusted (Tier-2) items.
pub const UNTRUSTED_WEIGHT: f64 = 0.65;
/// Maximum additive boost from log access frequency.
pub const MAX_ACCESS_BOOST: f64 = 0.15;
/// Maximum size in bytes for a user's pinned standing profile.
pub const PROFILE_MAX_BYTES: usize = 8 * 1024;
/// Maximum number of distilled lessons kept active per `(tenant, scope)` bucket.
pub const MAX_LESSONS_PER_SCOPE: usize = 40;
/// Default token budget (approximate, 4 chars/token) for packed retrieval output.
pub const DEFAULT_TOKEN_BUDGET: usize = 3_000;
/// Default top-k candidates returned by retrieval.
pub const DEFAULT_RETRIEVE_LIMIT: usize = 16;
/// Maximum graph expansion hops on read.
pub const MAX_GRAPH_HOPS: usize = 2;
/// Maximum neighbor edges pulled per seed node during graph expansion.
pub const MAX_EDGES_PER_SEED: usize = 10;
/// EMA smoothing factor for outcome feedback updates.
pub const FEEDBACK_EMA_ALPHA: f64 = 0.25;
/// Floor and ceiling for `feedback_weight`.
pub const MIN_FEEDBACK_WEIGHT: f64 = 0.15;
pub const MAX_FEEDBACK_WEIGHT: f64 = 2.0;
/// Cosine similarity threshold for near-duplicate assertion merging.
pub const DEDUP_COSINE_THRESHOLD: f64 = 0.92;
/// Cosine similarity threshold for flagging candidate conflicts on the same entity.
pub const CONFLICT_COSINE_THRESHOLD: f64 = 0.78;
/// Days of inactivity before cold untrusted assertions are archived.
pub const COLD_UNTRUSTED_DAYS: i64 = 30;
/// Maximum enrichment retry attempts before marking a job dead-lettered.
pub const MAX_ENRICHMENT_ATTEMPTS: i32 = 5;
/// Base retry backoff for failed enrichment jobs.
pub const ENRICHMENT_RETRY_BASE: Duration = Duration::from_secs(15);
/// Background curator poll interval (0 LLM calls when watermarks are clean).
pub const CURATOR_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Trust and confidentiality label carried by every memory item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustLabel {
    pub untrusted: bool,
    pub private: bool,
}

impl TrustLabel {
    pub fn join(self, other: Self) -> Self {
        Self { untrusted: self.untrusted || other.untrusted, private: self.private || other.private }
    }

    /// True if `self` has equal or higher factual authority than `other`
    /// (clean facts may supersede clean or untrusted; untrusted may only supersede untrusted).
    pub fn may_supersede(self, other: Self) -> bool {
        !self.untrusted || other.untrusted
    }
}

/// Caller identity and RLS context for every read operation.
#[derive(Clone, Debug)]
pub struct ReaderContext {
    pub tenant_id: String,
    pub person: String,
    pub principals: Vec<String>,
}

impl ReaderContext {
    pub fn for_person(tenant_id: impl Into<String>, person: impl Into<String>, groups: &[String]) -> Self {
        let person_str = person.into();
        let mut principals = Vec::with_capacity(1 + groups.len());
        principals.push(format!("person:{person_str}"));
        for g in groups {
            principals.push(format!("group:{g}"));
        }
        Self { tenant_id: tenant_id.into(), person: person_str, principals }
    }
}

/// A versioned source evidence record to ingest synchronously.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceInput {
    pub tenant_id: String,
    pub source_id: String,
    pub rev: i64,
    pub owner_id: Option<String>,
    /// CNF reader clauses (e.g. `[["person:alice"]]` or `[]` for org-wide).
    pub readers: Vec<Vec<String>>,
    pub label: TrustLabel,
    pub kind: String,
    pub text: String,
    pub metadata: Value,
    pub occurred_at: DateTime<Utc>,
}

/// An atomic assertion derived from one or more sources.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssertionInput {
    pub assertion_id: String,
    pub kind: String,
    pub statement: String,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub confidence: f64,
    pub embedding: Option<Vec<f32>>,
    pub model: Option<String>,
}

/// A canonical entity extracted from evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityInput {
    pub entity_type: String,
    pub canonical_name: String,
    pub external_id: Option<String>,
    pub aliases: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub model: Option<String>,
}

/// A typed directed edge between entities or assertions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeInput {
    pub edge_id: String,
    pub src_id: String,
    pub dst_id: String,
    pub rel_type: String,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub properties: Value,
}

/// Result returned by unified hybrid + temporal + graph retrieval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrievalResponse {
    pub hits: Vec<MemoryHit>,
    pub graph_facts: Vec<GraphFact>,
    pub conflicts: Vec<ConflictPair>,
    pub label: TrustLabel,
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryHit {
    pub id: String,
    pub record_kind: String,
    pub text: String,
    pub score: f64,
    pub label: TrustLabel,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub citations: Vec<String>,
    pub exact_match: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphFact {
    pub edge_id: String,
    pub src_id: String,
    pub src_name: String,
    pub rel_type: String,
    pub dst_id: String,
    pub dst_name: String,
    pub untrusted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConflictPair {
    pub assertion_a: String,
    pub statement_a: String,
    pub assertion_b: String,
    pub statement_b: String,
    pub edge_id: String,
}

/// Unified Postgres-backed memory engine owned by `Kernel`.
pub struct MemoryEngine {
    url: String,
    reader_role: String,
    client: Mutex<Option<Client>>,
}

impl MemoryEngine {
    pub fn new(database_url: &str) -> Self {
        let role = url::Url::parse(database_url).map(|u| u.username().to_string()).unwrap_or_default();
        Self { url: database_url.to_string(), reader_role: format!("{role}_reader"), client: Mutex::new(None) }
    }

    pub fn reader_role(&self) -> &str {
        &self.reader_role
    }

    pub(crate) async fn client(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Client>>> {
        let mut guard = self.client.lock().await;
        if guard.as_ref().is_none_or(|c| c.is_closed()) {
            *guard = Some(crate::log::connect(&self.url).await.context("connect memory engine to Postgres")?);
        }
        Ok(guard)
    }
}

/// Format a slice of f32 into pgvector's text literal `[0.1,0.2,...]`.
pub fn format_vector(v: &[f32]) -> String {
    let mut out = String::with_capacity(v.len() * 10 + 2);
    out.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        use std::fmt::Write;
        let _ = write!(&mut out, "{x}");
    }
    out.push(']');
    out
}

/// Intersect two CNF reader lists (logical AND): the result is readable only by
/// principals that satisfy *both* `a` and `b`.
pub fn intersect_readers(a: &[Vec<String>], b: &[Vec<String>]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = a.to_vec();
    for clause in b {
        if !out.contains(clause) {
            out.push(clause.clone());
        }
    }
    out
}
