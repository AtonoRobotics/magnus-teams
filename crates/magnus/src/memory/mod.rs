//! Unified Rust + Postgres memory engine (Phase 1-2: core schema, tenant+CNF
//! RLS store, sync ingest, fenced enrichment, and 0-LLM hybrid+graph retrieval).

pub mod curator;
pub mod enrich;
pub mod graph;
pub mod ingest;
pub mod retrieve;
pub mod schema;
pub mod store;

#[cfg(test)]
mod bench_tests;
#[cfg(test)]
mod curator_tests;
#[cfg(test)]
mod live_pg_test;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Fixed UUIDv5 namespace for deterministic canonical entity IDs.
const ENTITY_UUID_NAMESPACE: Uuid = Uuid::from_bytes([0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8]);

/// Taint lattice carried across sources, assertions, edges, and retrieval outputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaintLabel {
    pub untrusted: bool,
    pub private: bool,
}

impl TaintLabel {
    /// Least upper bound (logical OR) across multiple input sources.
    pub fn join(self, other: Self) -> Self {
        Self { untrusted: self.untrusted || other.untrusted, private: self.private || other.private }
    }
}

/// Versioned reference to a single `memory_sources` revision.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRef {
    pub source_id: String,
    pub rev: i64,
}

/// Shared read scope passed across retrieval and graph expansion helpers.
#[derive(Clone, Copy, Debug)]
pub struct ReadScope<'a> {
    pub tenant_id: &'a str,
    pub allow_untrusted: bool,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

/// Assertion category stored in `public.memory_assertions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionKind {
    Fact,
    Chunk,
    Lesson,
    Profile,
}

impl AssertionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Chunk => "chunk",
            Self::Lesson => "lesson",
            Self::Profile => "profile",
        }
    }
}

impl std::str::FromStr for AssertionKind {
    type Err = anyhow::Error;

    /// Inverse of `as_str`, for kinds read back from `memory_assertions.kind`.
    fn from_str(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "fact" => Ok(Self::Fact),
            "chunk" => Ok(Self::Chunk),
            "lesson" => Ok(Self::Lesson),
            "profile" => Ok(Self::Profile),
            other => anyhow::bail!("unknown assertion kind {other:?}"),
        }
    }
}

/// Input payload for synchronous source ingestion (`memory_sources`).
#[derive(Clone, Debug)]
pub struct SourceInput {
    pub tenant_id: String,
    pub source_id: String,
    pub rev: i64,
    pub owner_id: String,
    /// CNF reader policy (`Vec<Vec<String>>`), matching `public.knowledge`.
    pub readers: Vec<Vec<String>>,
    pub label: TaintLabel,
    pub text: String,
    pub occurred_at: DateTime<Utc>,
}

/// Derived assertion produced by async enrichment or distillation.
#[derive(Clone, Debug)]
pub struct AssertionInput {
    pub assertion_id: String,
    pub kind: AssertionKind,
    pub owner_id: String,
    pub statement: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub confidence: f32,
    pub embedding: Option<Vec<f32>>,
}

/// Thin canonical entity reference. `display_name` is stored per-edge on
/// RLS-protected `memory_edges`, never on the shared `memory_entities` row.
#[derive(Clone, Debug)]
pub struct EntityInput {
    pub entity_type: String,
    pub display_name: String,
    pub external_id: Option<String>,
}

/// Entity-to-entity graph edge carrying both `untrusted` and `private` flags.
#[derive(Clone, Debug)]
pub struct EdgeInput {
    pub edge_id: String,
    pub src: EntityInput,
    pub dst: EntityInput,
    pub rel_type: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub properties: Value,
}

/// Normalize text for exact-content sync deduplication and canonical IDs.
pub fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// SHA-256 hex digest of normalized source text for sync idempotency.
pub fn content_hash(text: &str) -> String {
    let norm = normalize_text(text);
    hex::encode(Sha256::digest(norm.as_bytes()))
}

/// Deterministic canonical entity ID: authoritative `external_id` takes precedence
/// over normalized `(entity_type, display_name)`.
pub fn canonical_entity_id(tenant_id: &str, entity: &EntityInput) -> String {
    let et = normalize_text(&entity.entity_type);
    let key = match entity.external_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(ext) => format!("{tenant_id}\0ext\0{et}\0{}", ext.to_lowercase()),
        None => format!("{tenant_id}\0name\0{et}\0{}", normalize_text(&entity.display_name)),
    };
    Uuid::new_v5(&ENTITY_UUID_NAMESPACE, key.as_bytes()).to_string()
}

/// Canonicalize a CNF reader policy so JSONB equality is independent of clause
/// or principal ordering.
pub fn canonicalize_readers(policy: &[Vec<String>]) -> Vec<Vec<String>> {
    let mut clauses: Vec<Vec<String>> = policy
        .iter()
        .map(|clause| {
            let mut c = clause.clone();
            c.sort();
            c.dedup();
            c
        })
        .collect();
    clauses.sort();
    clauses.dedup();
    clauses
}

/// Strictest CNF reader intersection across multiple input policies (AND of CNF
/// policies is the canonicalized union of their clauses).
pub fn intersect_readers(policies: &[Vec<Vec<String>>]) -> Vec<Vec<String>> {
    let mut combined: Vec<Vec<String>> = Vec::new();
    for policy in policies {
        combined.extend(policy.iter().cloned());
    }
    canonicalize_readers(&combined)
}

/// True when every principal set that satisfies the CNF policy `narrower` also
/// satisfies `wider`: each clause of `wider` contains some clause of `narrower`.
/// An empty `wider` (no clause) admits every reader.
pub fn readers_cover(wider: &[Vec<String>], narrower: &[Vec<String>]) -> bool {
    wider.iter().all(|w| narrower.iter().any(|n| n.iter().all(|p| w.contains(p))))
}

/// Format an embedding slice as a pgvector literal `[v0,v1,...]` after verifying
/// fixed dimension `expected_dim`.
pub fn format_halfvec_literal(embedding: &[f32], expected_dim: usize) -> anyhow::Result<String> {
    anyhow::ensure!(embedding.len() == expected_dim, "embedding dimension mismatch: got {}, expected {expected_dim}", embedding.len());
    let mut out = String::with_capacity(embedding.len() * 10 + 2);
    out.push('[');
    for (idx, v) in embedding.iter().enumerate() {
        anyhow::ensure!(v.is_finite(), "embedding contains non-finite value at index {idx}");
        if idx > 0 {
            out.push(',');
        }
        use std::fmt::Write;
        let _ = write!(&mut out, "{v}");
    }
    out.push(']');
    Ok(out)
}
