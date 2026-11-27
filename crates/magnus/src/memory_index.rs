//! Generalised Postgres hybrid memory index, deterministic RRF ranking,
//! write caps and deduplication (W2.1, M6, M7, N11, O1).
//!
//! Reuses the same Postgres `tsvector` + untyped `vector` + `model` + forced RLS
//! architecture established in `knowledge.rs`. Recall does not read it: it
//! searches each person's memory where it is kept (`recall::Recall::search`).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Default fixed `halfvec(N)` dimension for the unified Postgres HNSW index.
pub const DEFAULT_EMBEDDING_DIM: usize = 1536;
/// Maximum `halfvec(N)` dimension supported by `pgvector` HNSW indexes.
pub const MAX_EMBEDDING_DIM: usize = 4000;
/// Maximum byte budget for pinned `kind = 'profile'` entries per user.
pub const PROFILE_MAX_BYTES: usize = 8192;
/// Default cap on active distilled `kind = 'lesson'` assertions per `(tenant_id, owner_id)`.
pub const DEFAULT_MAX_LESSONS_PER_SCOPE: usize = 64;
/// Largest accepted `[memory] max_lessons_per_scope`.
pub const MAX_LESSONS_PER_SCOPE_LIMIT: usize = 4096;

/// Validated `pgvector >= 0.8.0` HNSW iterative scan mode.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HnswIterativeScan {
    #[default]
    RelaxedOrder,
    StrictOrder,
}

impl HnswIterativeScan {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            Self::RelaxedOrder => "relaxed_order",
            Self::StrictOrder => "strict_order",
        }
    }
}

fn default_embedding_dim() -> usize {
    DEFAULT_EMBEDDING_DIM
}

fn default_max_lessons_per_scope() -> usize {
    DEFAULT_MAX_LESSONS_PER_SCOPE
}

/// Typed startup `[memory]` configuration (validated at startup, takes effect on restart).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryConfig {
    /// O1: Personal memory defaults to Postgres FTS-only (`false`) unless the owner
    /// configures a local/self-hosted embedding server or explicitly opts in.
    #[serde(default)]
    pub personal_embeddings: bool,
    /// Fixed `halfvec(N)` embedding dimension for `memory_assertions`.
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: usize,
    /// `pgvector >= 0.8.0` HNSW iterative scan mode.
    #[serde(default)]
    pub hnsw_iterative_scan: HnswIterativeScan,
    /// Cap on active distilled lessons per `(tenant_id, owner_id)`; the curator
    /// archives the least-used excess when a new lesson is distilled.
    #[serde(default = "default_max_lessons_per_scope")]
    pub max_lessons_per_scope: usize,
    /// Tenant this appliance's people recall from in the unified store. One
    /// organisation per appliance today, so one tenant; set here, never in code.
    #[serde(default = "default_tenant_id")]
    pub tenant_id: String,
}

/// Tenant used when `[memory] tenant_id` is not set.
pub const DEFAULT_TENANT_ID: &str = "default";
/// Longest accepted `[memory] tenant_id`, in characters.
pub const MAX_TENANT_ID_CHARS: usize = 64;

fn default_tenant_id() -> String {
    DEFAULT_TENANT_ID.to_string()
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            personal_embeddings: false,
            embedding_dim: default_embedding_dim(),
            hnsw_iterative_scan: HnswIterativeScan::default(),
            max_lessons_per_scope: default_max_lessons_per_scope(),
            tenant_id: default_tenant_id(),
        }
    }
}

impl MemoryConfig {
    /// Validate `[memory]` settings at startup (`Config::load`).
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.embedding_dim >= 1 && self.embedding_dim <= MAX_EMBEDDING_DIM,
            "[memory] embedding_dim must be between 1 and {MAX_EMBEDDING_DIM}, got {}",
            self.embedding_dim
        );
        anyhow::ensure!(
            self.max_lessons_per_scope >= 1 && self.max_lessons_per_scope <= MAX_LESSONS_PER_SCOPE_LIMIT,
            "[memory] max_lessons_per_scope must be between 1 and {MAX_LESSONS_PER_SCOPE_LIMIT}, got {}",
            self.max_lessons_per_scope
        );
        anyhow::ensure!(
            !self.tenant_id.is_empty()
                && self.tenant_id.chars().count() <= MAX_TENANT_ID_CHARS
                && self.tenant_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "[memory] tenant_id must be 1 to {MAX_TENANT_ID_CHARS} letters, digits, '_' or '-', got {:?}",
            self.tenant_id
        );
        Ok(())
    }
}

/// Named constants with explicit units (AGENTS.md Code Standard).
pub const EMBED_TIMEOUT: Duration = Duration::from_millis(500);
pub const MAX_MEMORY_WRITES_PER_RUN: usize = 12;
/// A memory is one short sentence about the person. The longest the
/// person-taught facts in production ran was under 400 characters; the
/// longer entries were run checkpoints and status reports.
pub const MAX_MEMORY_FACT_CHARS: usize = 300;
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

/// Whether `fact` repeats `existing`: the same words whatever the case,
/// spacing or punctuation, at any length, or a near duplicate when long
/// enough for word overlap to mean anything.
pub fn same_fact(fact: &str, existing: &str) -> bool {
    let words = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
    let (a, b) = (words(fact), words(existing));
    !a.is_empty() && (a == b || (a.len() >= 4 && is_near_duplicate(fact, existing, None, None)))
}

/// Why `fact` may not be remembered, if it may not: memory keeps short
/// facts the person taught, so not an empty one, not a paragraph (a run's
/// checkpoint or report), and nothing from a run the person said nothing
/// in, which is the system's dialog with itself (a review, a heartbeat, a
/// schedule's run).
pub fn fact_refusal(fact: &str, person_spoke: bool) -> Option<String> {
    if fact.is_empty() {
        return Some("memory_write needs a fact.".into());
    }
    let n = fact.chars().count();
    if n > MAX_MEMORY_FACT_CHARS {
        return Some(format!("Not remembered: a memory is one short sentence about the person (at most {MAX_MEMORY_FACT_CHARS} characters), and this is {n}. Run notes, checkpoints and reports are not memories."));
    }
    if !person_spoke {
        return Some(
            "Not remembered: memory keeps what the person told Magnus, and the person said nothing in this run (a review, a schedule's run or a check Magnus started itself)."
                .into(),
        );
    }
    None
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
    fn the_same_fact_is_one_memory_at_any_length() {
        assert!(same_fact("Calls me Mags.", "calls me mags"), "short facts repeat too");
        assert!(same_fact("The person prefers to call the assistant Mags.", "The person prefers to call the assistant Mags"));
        assert!(!same_fact("Calls me Mags.", "Calls me Sam."));
        assert!(!same_fact("", ""), "nothing is not a fact");
    }

    #[test]
    fn memory_refuses_paragraphs_and_the_systems_own_runs() {
        assert_eq!(fact_refusal("The person is allergic to peanuts.", true), None);
        let checkpoint = "RUN 1 CHECKPOINT phase=awaiting_continuity_check ".repeat(8);
        assert!(fact_refusal(&checkpoint, true).is_some_and(|r| r.contains("short sentence")), "a run's checkpoint is not a memory");
        assert!(fact_refusal("The person's latest review found a promotion.", false).is_some_and(|r| r.contains("said nothing")), "a review's report is not a memory");
        assert!(fact_refusal("", true).is_some());
    }

    /// A config written before recall moved to the store still loads.
    #[test]
    fn an_old_memory_mode_setting_still_loads() {
        let cfg: MemoryConfig = toml::from_str("mode = \"legacy\"\n[person_modes]\nalice = \"v2\"\n").unwrap();
        assert!(!cfg.personal_embeddings);
    }
}
