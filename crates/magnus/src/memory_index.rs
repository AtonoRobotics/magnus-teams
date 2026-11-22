//! Generalised Postgres hybrid memory index, deterministic RRF ranking,
//! write caps, deduplication, and single rollback switch (W2.1, W2.4, M6, M7, N11, N13, O1).
//!
//! Reuses the same Postgres `tsvector` + untyped `vector` + `model` + forced RLS
//! architecture established in `knowledge.rs`, while keeping `public.events` and
//! `public.content` untouched so flipping `[memory] mode = "legacy"` rolls back
//! instantly with zero data loss.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// Single rollback switch for Wave 2/3 memory upgrades (W2.4, N13).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    #[default]
    Legacy,
    Shadow,
    V2,
}

/// Typed startup `[memory]` configuration (validated at startup, takes effect on restart).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryConfig {
    #[serde(default)]
    pub mode: MemoryMode,
    /// O1: Personal memory defaults to Postgres FTS-only (`false`) unless the owner
    /// configures a local/self-hosted embedding server or explicitly opts in.
    #[serde(default)]
    pub personal_embeddings: bool,
    /// Optional per-person canary overrides in config file (takes effect on restart, N13).
    #[serde(default)]
    pub person_modes: BTreeMap<String, MemoryMode>,
}

impl MemoryConfig {
    pub fn mode_for(&self, person: &str) -> MemoryMode {
        self.person_modes.get(person).copied().unwrap_or(self.mode)
    }
}

/// Named constants with explicit units (AGENTS.md Code Standard).
pub const EMBED_TIMEOUT: Duration = Duration::from_millis(500);
pub const MAX_MEMORY_WRITES_PER_RUN: usize = 12;
pub const DEDUP_SIMILARITY_THRESHOLD: f32 = 0.92;
pub const DEDUP_TOKEN_OVERLAP_THRESHOLD: f32 = 0.85;
pub const RRF_K: f64 = 60.0;
pub const TIER2_WEIGHT: f64 = 0.65;
pub const MAX_ACCESS_BOOST: f64 = 0.15;
pub const RECENCY_HALF_LIFE_DAYS: f64 = 30.0;
pub const TIER2_COLD_DAYS: i64 = 30;

/// Additive Postgres DDL for derived hybrid memory index and discardable access stats.
/// Untyped `vector` + `model` matches `knowledge.rs` (M6); forced RLS isolates per person.
pub const MEMORY_INDEX_MIGRATION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS public.memory_index (
    id          TEXT PRIMARY KEY,
    person      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    untrusted   BOOLEAN NOT NULL DEFAULT FALSE,
    private     BOOLEAN NOT NULL DEFAULT FALSE,
    sources     JSONB NOT NULL DEFAULT '[]'::jsonb,
    body_tsv    TSVECTOR NOT NULL,
    embedding   vector,
    model       TEXT,
    created_at  TIMESTAMPTZ NOT NULL,
    archived    BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS memory_index_person_tsv_idx ON public.memory_index USING GIN (body_tsv);
CREATE INDEX IF NOT EXISTS memory_index_person_created_idx ON public.memory_index (person, created_at DESC);
ALTER TABLE public.memory_index ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.memory_index FORCE ROW LEVEL SECURITY;

CREATE TABLE IF NOT EXISTS public.memory_access_stats (
    id            TEXT PRIMARY KEY,
    person        TEXT NOT NULL,
    last_accessed TIMESTAMPTZ NOT NULL,
    access_count  INTEGER NOT NULL DEFAULT 0
);
ALTER TABLE public.memory_access_stats ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.memory_access_stats FORCE ROW LEVEL SECURITY;
"#;

/// Tenant scope for the unified hybrid memory index (Phase 2 Teams Redesign).
/// Strictly separates private per-person memory (`person:<id>`) from shared
/// workspace memory (`workspace:<id>`), enforced via Postgres RLS.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum MemoryScope {
    Person(String),
    Workspace(String),
}

impl MemoryScope {
    pub fn key(&self) -> String {
        match self {
            MemoryScope::Person(id) => format!("person:{id}"),
            MemoryScope::Workspace(id) => format!("workspace:{id}"),
        }
    }

    pub fn is_workspace(&self) -> bool {
        matches!(self, MemoryScope::Workspace(_))
    }
}

/// Additive Postgres DDL extending `public.memory_index` with `scope` for
/// unified `person:<id>` + `workspace:<id>` RLS isolation.
pub const WORKSPACE_MEMORY_MIGRATION_SQL: &str = r#"
ALTER TABLE public.memory_index ADD COLUMN IF NOT EXISTS scope TEXT;
UPDATE public.memory_index SET scope = 'person:' || person WHERE scope IS NULL;
CREATE INDEX IF NOT EXISTS memory_index_scope_created_idx ON public.memory_index (scope, created_at DESC);
"#;

/// Input candidate for deterministic hybrid RRF + decay + trust ranking.
#[derive(Clone, Debug)]
pub struct RankedCandidate {
    pub id: String,
    pub fts_rank: Option<usize>,
    pub vec_rank: Option<usize>,
    pub untrusted: bool,
    pub age_days: f64,
    pub access_count: u32,
}

/// Compute deterministic hybrid score combining Reciprocal Rank Fusion (FTS + vector),
/// exponential recency decay, capped access boost (`MAX_ACCESS_BOOST`, m2),
/// and Tier-2 untrusted down-weighting (`TIER2_WEIGHT`).
pub fn hybrid_score(c: &RankedCandidate) -> f64 {
    let fts = c.fts_rank.map_or(0.0, |r| 1.0 / (RRF_K + (r as f64) + 1.0));
    let vec = c.vec_rank.map_or(0.0, |r| 1.0 / (RRF_K + (r as f64) + 1.0));
    let base = fts + vec;
    if base <= 0.0 {
        return 0.0;
    }
    let decay = 0.5_f64.powf((c.age_days.max(0.0)) / RECENCY_HALF_LIFE_DAYS);
    let recency_factor = 0.5 + 0.5 * decay;
    let access_boost = ((c.access_count as f64 + 1.0).ln() * 0.05).min(MAX_ACCESS_BOOST);
    let trust_factor = if c.untrusted { TIER2_WEIGHT } else { 1.0 };
    base * recency_factor * (1.0 + access_boost) * trust_factor
}

/// Deterministic Jaccard token-overlap similarity used for near-duplicate
/// rejection when running FTS-only without embeddings (N11, O1).
pub fn token_overlap_similarity(a: &str, b: &str) -> f32 {
    let tokens = |s: &str| -> HashSet<String> { s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 2).map(|w| w.to_ascii_lowercase()).collect() };
    let sa = tokens(a);
    let sb = tokens(b);
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    inter / union
}

/// Cosine similarity between two equal-length embeddings.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Returns `true` if `candidate_text` is a near-duplicate of `existing_text`
/// using cosine similarity when both embeddings are present, or Jaccard token
/// overlap (`DEDUP_TOKEN_OVERLAP_THRESHOLD`) under FTS-only mode (N11).
pub fn is_near_duplicate(candidate_text: &str, existing_text: &str, candidate_emb: Option<&[f32]>, existing_emb: Option<&[f32]>) -> bool {
    if let (Some(a), Some(b)) = (candidate_emb, existing_emb) {
        if a.len() == b.len() && !a.is_empty() {
            return cosine_similarity(a, b) >= DEDUP_SIMILARITY_THRESHOLD;
        }
    }
    token_overlap_similarity(candidate_text, existing_text) >= DEDUP_TOKEN_OVERLAP_THRESHOLD
}

/// SQL statement for cascading person erasure across `memory_index` and
/// `memory_access_stats` (M7).
pub fn erasure_sql_statements() -> [&'static str; 2] {
    ["DELETE FROM public.memory_index WHERE person = $1", "DELETE FROM public.memory_access_stats WHERE person = $1"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_facts_outrank_equal_untrusted_facts_and_access_boost_is_capped() {
        let clean = RankedCandidate { id: "clean".into(), fts_rank: Some(0), vec_rank: Some(0), untrusted: false, age_days: 1.0, access_count: 0 };
        let untrusted = RankedCandidate { id: "tainted".into(), fts_rank: Some(0), vec_rank: Some(0), untrusted: true, age_days: 1.0, access_count: 100_000 };
        assert!(hybrid_score(&clean) > hybrid_score(&untrusted), "capped access boost must not let untrusted fact outrank clean fact");
    }

    #[test]
    fn dedup_works_with_and_without_embeddings() {
        assert!(is_near_duplicate("User prefers concise bullet summaries in status emails", "User prefers concise bullet summaries in status emails!", None, None));
        assert!(!is_near_duplicate("User prefers concise summaries", "Office moved to Austin Texas", None, None));
        assert!(is_near_duplicate("a", "b", Some(&[1.0, 0.0]), Some(&[0.99, 0.1])));
        assert!(!is_near_duplicate("a", "b", Some(&[1.0, 0.0]), Some(&[0.0, 1.0])));
    }

    #[test]
    fn config_mode_supports_per_person_canary_and_defaults_legacy() {
        let mut cfg = MemoryConfig::default();
        assert_eq!(cfg.mode_for("alice"), MemoryMode::Legacy);
        assert!(!cfg.personal_embeddings);
        cfg.person_modes.insert("alice".into(), MemoryMode::V2);
        assert_eq!(cfg.mode_for("alice"), MemoryMode::V2);
        assert_eq!(cfg.mode_for("bob"), MemoryMode::Legacy);
    }
}
