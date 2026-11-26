//! Unified hybrid + temporal + graph retrieval engine.
//!
//! Runs every read inside `SET LOCAL ROLE {role}_reader` with `magnus.tenant`
//! and `magnus.principals` set, combining:
//! 1. Exact ID / external-ID / entity fast-path (explicit top precedence).
//! 2. SQL FTS (`ts_rank_cd`) + `pgvector` iterative HNSW cosine fused via RRF.
//! 3. Deterministic temporal candidate expansion when the query specifies a date/year.
//! 4. Bounded 1–2 hop RLS-filtered graph expansion (`graph::expand_neighborhood`).
//! 5. Read-time visibility-scoped supersession & conflict resolution.
//! 6. Recency decay, log access boost, feedback weight, trust weight, and token budgeting.

use crate::memory::graph::{expand_neighborhood, resolve_supersession_and_conflicts};
use crate::memory::{
    format_vector, MemoryEngine, MemoryHit, ReaderContext, RetrievalResponse, TrustLabel, DEFAULT_RETRIEVE_LIMIT, DEFAULT_TOKEN_BUDGET, MAX_ACCESS_BOOST, MAX_GRAPH_HOPS,
    RECENCY_HALF_LIFE_DAYS, RRF_K, UNTRUSTED_WEIGHT,
};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use tokio_postgres::Transaction;

/// Parameters for a unified retrieval call.
#[derive(Clone, Debug)]
pub struct RetrieveQuery {
    pub text: String,
    pub embedding: Option<Vec<f32>>,
    pub model: Option<String>,
    pub limit: usize,
    pub token_budget: usize,
    pub graph_hops: usize,
    pub kind_filter: Option<String>,
}

impl RetrieveQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), embedding: None, model: None, limit: DEFAULT_RETRIEVE_LIMIT, token_budget: DEFAULT_TOKEN_BUDGET, graph_hops: MAX_GRAPH_HOPS, kind_filter: None }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    id: String,
    record_kind: String,
    text: String,
    label: TrustLabel,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    access_count: i32,
    feedback_weight: f64,
    fts_rank: Option<usize>,
    vec_rank: Option<usize>,
    exact_match: bool,
    temporal_match: bool,
}

/// Deterministically extract a `[start, end)` UTC window from a query containing
/// an ISO date (`2026-03-15`), year-month (`2026-03`), or 4-digit year (`2026`).
pub fn extract_temporal_window(query: &str) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    static DATE_RE: OnceLock<Regex> = OnceLock::new();
    static YM_RE: OnceLock<Regex> = OnceLock::new();
    static YEAR_RE: OnceLock<Regex> = OnceLock::new();

    if let Ok(re) =
        DATE_RE.get_or_init(|| Regex::new(r"\b(20\d{2})-(\d{2})-(\d{2})\b").unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| unreachable!()))).captures(query).ok_or(())
    {
        if let (Ok(y), Ok(m), Ok(d)) = (re[1].parse::<i32>(), re[2].parse::<u32>(), re[3].parse::<u32>()) {
            if let Some(day) = NaiveDate::from_ymd_opt(y, m, d) {
                let start = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0)?);
                let end = start + chrono::Duration::days(1);
                return Some((start, end));
            }
        }
    }
    if let Some(re) = YM_RE.get_or_init(|| Regex::new(r"\b(20\d{2})-(\d{2})\b").unwrap_or_else(|_| unreachable!())).captures(query) {
        if let (Ok(y), Ok(m)) = (re[1].parse::<i32>(), re[2].parse::<u32>()) {
            if let Some(first) = NaiveDate::from_ymd_opt(y, m, 1) {
                let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                if let Some(next_first) = NaiveDate::from_ymd_opt(ny, nm, 1) {
                    return Some((Utc.from_utc_datetime(&first.and_hms_opt(0, 0, 0)?), Utc.from_utc_datetime(&next_first.and_hms_opt(0, 0, 0)?)));
                }
            }
        }
    }
    if let Some(re) = YEAR_RE.get_or_init(|| Regex::new(r"\b(20\d{2})\b").unwrap_or_else(|_| unreachable!())).captures(query) {
        if let Ok(y) = re[1].parse::<i32>() {
            if let (Some(start), Some(end)) = (NaiveDate::from_ymd_opt(y, 1, 1), NaiveDate::from_ymd_opt(y + 1, 1, 1)) {
                return Some((Utc.from_utc_datetime(&start.and_hms_opt(0, 0, 0)?), Utc.from_utc_datetime(&end.and_hms_opt(0, 0, 0)?)));
            }
        }
    }
    None
}

/// Extract exact-lookup tokens (IDs like `INV-1042`, `kb_foo`, emails, quoted phrases).
pub fn extract_exact_tokens(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in query.split_whitespace() {
        let tok = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '@' && c != '.');
        let has_digit = tok.chars().any(|c| c.is_ascii_digit());
        let has_sep = tok.contains('-') || tok.contains('_') || tok.contains('@');
        if tok.len() >= 4 && ((has_digit && has_sep) || tok.contains('@')) {
            out.push(tok.to_string());
        }
    }
    out
}

/// Compute hybrid RRF + recency + access + feedback + trust score.
fn score_candidate(c: &Candidate, now: DateTime<Utc>) -> f64 {
    let rrf_fts = c.fts_rank.map(|r| 1.0 / (RRF_K + (r as f64) + 1.0)).unwrap_or(0.0);
    let rrf_vec = c.vec_rank.map(|r| 1.0 / (RRF_K + (r as f64) + 1.0)).unwrap_or(0.0);
    let base_rrf = rrf_fts + rrf_vec + if c.temporal_match { 0.01 } else { 0.0 };
    let exact_bonus = if c.exact_match { 1.0 } else { 0.0 };

    let age_days = (now - c.created_at).num_seconds().max(0) as f64 / 86_400.0;
    let recency = 0.5 + 0.5 * 0.5_f64.powf(age_days / RECENCY_HALF_LIFE_DAYS);
    let access_boost = 1.0 + (0.05 * ((c.access_count.max(0) as f64) + 1.0).ln()).min(MAX_ACCESS_BOOST);
    let trust_weight = if c.label.untrusted { UNTRUSTED_WEIGHT } else { 1.0 };

    (exact_bonus + base_rrf) * recency * access_boost * c.feedback_weight.max(0.1) * trust_weight
}

fn fts_terms(query: &str) -> Vec<String> {
    query.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(String::from).collect()
}

impl MemoryEngine {
    /// Execute unified hybrid + temporal + graph retrieval under the caller's Postgres RLS.
    pub async fn retrieve(&self, ctx: &ReaderContext, q: &RetrieveQuery) -> Result<RetrievalResponse> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await.context("begin retrieve transaction")?;

        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader_role())).await?;
        tx.execute("SELECT set_config('magnus.tenant', $1, true)", &[&ctx.tenant_id]).await?;
        tx.execute("SELECT set_config('magnus.principals', $1, true)", &[&serde_json::to_string(&ctx.principals)?]).await?;
        // Enable pgvector 0.8+ iterative HNSW scan so selective RLS policies do not starve top-k.
        let _ = tx.batch_execute("SET LOCAL hnsw.iterative_scan = relaxed_order").await;

        let fetch_k = (q.limit.max(1) * 3).min(120) as i64;
        let mut candidates: HashMap<String, Candidate> = HashMap::new();
        let mut entity_seeds: Vec<String> = Vec::new();

        // 1. Exact ID / external-ID / entity fast-path.
        let exact_tokens = extract_exact_tokens(&q.text);
        if !exact_tokens.is_empty() {
            collect_exact(&tx, &ctx.tenant_id, &exact_tokens, &mut candidates, &mut entity_seeds).await?;
        }

        // 2. Lexical FTS (`ts_rank_cd`) over assertions and live sources.
        let terms = fts_terms(&q.text);
        if !terms.is_empty() {
            let tsquery = terms.join(" | ");
            collect_fts(&tx, &ctx.tenant_id, &tsquery, q.kind_filter.as_deref(), fetch_k, &mut candidates).await?;
            collect_entity_name_matches(&tx, &ctx.tenant_id, &terms, &mut entity_seeds).await?;
        }

        // 3. Vector similarity (`pgvector` cosine `<=>`) when query embedding is provided.
        if let Some(vec) = &q.embedding {
            let vec_lit = format_vector(vec);
            collect_vector(&tx, &ctx.tenant_id, &vec_lit, q.model.as_deref(), q.kind_filter.as_deref(), fetch_k, &mut candidates).await?;
        }

        // 4. Deterministic temporal candidate expansion.
        if let Some((win_start, win_end)) = extract_temporal_window(&q.text) {
            collect_temporal(&tx, &ctx.tenant_id, win_start, win_end, fetch_k, &mut candidates).await?;
        }

        // 4b. Empty query ("what do you remember?"): return latest active authorized assertions.
        if terms.is_empty() && q.embedding.is_none() && exact_tokens.is_empty() {
            collect_recent(&tx, &ctx.tenant_id, q.kind_filter.as_deref(), fetch_k, &mut candidates).await?;
        }

        // 5. Read-time visibility-scoped supersession & conflict resolution.
        let assertion_ids: Vec<String> = candidates.values().filter(|c| c.record_kind != "source").map(|c| c.id.clone()).collect();
        let (superseded, conflicts) = resolve_supersession_and_conflicts(&tx, &ctx.tenant_id, &assertion_ids).await?;

        candidates.retain(|id, _| !superseded.contains(id));

        // 6. Score, sort, and pack within token budget.
        let now = Utc::now();
        let mut scored: Vec<(f64, Candidate)> = candidates.into_values().map(|c| (score_candidate(&c, now), c)).collect();
        scored.sort_by(|a, b| b.1.exact_match.cmp(&a.1.exact_match).then_with(|| b.0.total_cmp(&a.0)).then_with(|| b.1.created_at.cmp(&a.1.created_at)));

        let mut hits = Vec::new();
        let mut used_chars = 0usize;
        let char_budget = q.token_budget.saturating_mul(4);
        let mut label = TrustLabel::default();
        let mut all_source_ids: HashSet<String> = HashSet::new();
        let mut graph_seeds: Vec<String> = entity_seeds;

        for (score, c) in scored.into_iter().take(q.limit) {
            let citations = if c.record_kind == "source" { vec![c.id.clone()] } else { fetch_citations(&tx, &ctx.tenant_id, &c.id).await? };
            let entry_len = c.text.len() + 64;
            if !hits.is_empty() && used_chars + entry_len > char_budget {
                break;
            }
            used_chars += entry_len;
            label = label.join(c.label);
            for cid in &citations {
                all_source_ids.insert(cid.clone());
            }
            graph_seeds.push(c.id.clone());
            hits.push(MemoryHit {
                id: c.id,
                record_kind: c.record_kind,
                text: c.text,
                score,
                label: c.label,
                valid_from: c.valid_from,
                valid_to: c.valid_to,
                citations,
                exact_match: c.exact_match,
            });
        }

        // 7. Bounded RLS-filtered graph expansion.
        graph_seeds.sort();
        graph_seeds.dedup();
        let graph_facts = expand_neighborhood(&tx, &ctx.tenant_id, &graph_seeds, q.graph_hops).await?;
        for gf in &graph_facts {
            if gf.untrusted {
                label.untrusted = true;
            }
        }

        tx.commit().await.context("commit retrieve transaction")?;

        let mut source_ids: Vec<String> = all_source_ids.into_iter().collect();
        source_ids.sort();

        Ok(RetrievalResponse { hits, graph_facts, conflicts, label, source_ids })
    }
}

async fn collect_exact(tx: &Transaction<'_>, tenant_id: &str, tokens: &[String], out: &mut HashMap<String, Candidate>, entity_seeds: &mut Vec<String>) -> Result<()> {
    for tok in tokens {
        let pattern = format!("%{tok}%");
        let a_rows = tx
            .query(
                "SELECT assertion_id, kind, statement, untrusted, private, valid_from, valid_to,
                        created_at, access_count, feedback_weight
                 FROM public.memory_assertions
                 WHERE tenant_id = $1 AND (assertion_id = $2 OR statement ILIKE $3)
                 LIMIT 10",
                &[&tenant_id, tok, &pattern],
            )
            .await?;
        for r in a_rows {
            let id: String = r.get(0);
            out.entry(id.clone()).and_modify(|c| c.exact_match = true).or_insert(Candidate {
                id,
                record_kind: r.get(1),
                text: r.get(2),
                label: TrustLabel { untrusted: r.get(3), private: r.get(4) },
                valid_from: r.get(5),
                valid_to: r.get(6),
                created_at: r.get(7),
                access_count: r.get(8),
                feedback_weight: r.get(9),
                fts_rank: Some(0),
                vec_rank: None,
                exact_match: true,
                temporal_match: false,
            });
        }

        let s_rows = tx
            .query(
                "SELECT source_id, text, untrusted, private, occurred_at, created_at
                 FROM public.memory_sources
                 WHERE tenant_id = $1 AND (source_id = $2 OR text ILIKE $3)
                 ORDER BY rev DESC
                 LIMIT 10",
                &[&tenant_id, tok, &pattern],
            )
            .await?;
        for r in s_rows {
            let id: String = r.get(0);
            out.entry(id.clone()).and_modify(|c| c.exact_match = true).or_insert(Candidate {
                id,
                record_kind: "source".into(),
                text: r.get(1),
                label: TrustLabel { untrusted: r.get(2), private: r.get(3) },
                valid_from: Some(r.get(4)),
                valid_to: None,
                created_at: r.get(5),
                access_count: 0,
                feedback_weight: 1.0,
                fts_rank: Some(0),
                vec_rank: None,
                exact_match: true,
                temporal_match: false,
            });
        }

        let e_rows = tx
            .query(
                "SELECT entity_id FROM public.memory_entities
                 WHERE tenant_id = $1 AND (external_id = $2 OR norm_name = lower($2))
                 LIMIT 5",
                &[&tenant_id, tok],
            )
            .await?;
        for r in e_rows {
            entity_seeds.push(r.get(0));
        }
    }
    Ok(())
}

async fn collect_fts(tx: &Transaction<'_>, tenant_id: &str, tsquery: &str, kind_filter: Option<&str>, limit: i64, out: &mut HashMap<String, Candidate>) -> Result<()> {
    let a_rows = tx
        .query(
            "SELECT assertion_id, kind, statement, untrusted, private, valid_from, valid_to,
                    created_at, access_count, feedback_weight
             FROM public.memory_assertions
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR kind = $2)
               AND tsv @@ to_tsquery('simple', $3)
             ORDER BY ts_rank_cd(tsv, to_tsquery('simple', $3)) DESC, created_at DESC
             LIMIT $4",
            &[&tenant_id, &kind_filter, &tsquery, &limit],
        )
        .await?;
    for (rank, r) in a_rows.into_iter().enumerate() {
        let id: String = r.get(0);
        out.entry(id.clone()).and_modify(|c| c.fts_rank = Some(c.fts_rank.map_or(rank, |prev| prev.min(rank)))).or_insert(Candidate {
            id,
            record_kind: r.get(1),
            text: r.get(2),
            label: TrustLabel { untrusted: r.get(3), private: r.get(4) },
            valid_from: r.get(5),
            valid_to: r.get(6),
            created_at: r.get(7),
            access_count: r.get(8),
            feedback_weight: r.get(9),
            fts_rank: Some(rank),
            vec_rank: None,
            exact_match: false,
            temporal_match: false,
        });
    }

    if kind_filter.is_none() || kind_filter == Some("source") {
        let s_rows = tx
            .query(
                "SELECT DISTINCT ON (source_id) source_id, text, untrusted, private, occurred_at, created_at
                 FROM public.memory_sources
                 WHERE tenant_id = $1 AND tsv @@ to_tsquery('simple', $2)
                 ORDER BY source_id, rev DESC, ts_rank_cd(tsv, to_tsquery('simple', $2)) DESC
                 LIMIT $3",
                &[&tenant_id, &tsquery, &limit],
            )
            .await?;
        for (rank, r) in s_rows.into_iter().enumerate() {
            let id: String = r.get(0);
            out.entry(id.clone()).and_modify(|c| c.fts_rank = Some(c.fts_rank.map_or(rank, |prev| prev.min(rank)))).or_insert(Candidate {
                id,
                record_kind: "source".into(),
                text: r.get(1),
                label: TrustLabel { untrusted: r.get(2), private: r.get(3) },
                valid_from: Some(r.get(4)),
                valid_to: None,
                created_at: r.get(5),
                access_count: 0,
                feedback_weight: 1.0,
                fts_rank: Some(rank),
                vec_rank: None,
                exact_match: false,
                temporal_match: false,
            });
        }
    }
    Ok(())
}

async fn collect_entity_name_matches(tx: &Transaction<'_>, tenant_id: &str, terms: &[String], entity_seeds: &mut Vec<String>) -> Result<()> {
    for term in terms.iter().take(6) {
        if term.len() < 3 {
            continue;
        }
        let pattern = format!("%{term}%");
        let rows = tx
            .query(
                "SELECT entity_id FROM public.memory_entities
                 WHERE tenant_id = $1 AND norm_name LIKE $2
                 LIMIT 5",
                &[&tenant_id, &pattern],
            )
            .await?;
        for r in rows {
            entity_seeds.push(r.get(0));
        }
    }
    Ok(())
}

async fn collect_vector(
    tx: &Transaction<'_>,
    tenant_id: &str,
    vec_lit: &str,
    model: Option<&str>,
    kind_filter: Option<&str>,
    limit: i64,
    out: &mut HashMap<String, Candidate>,
) -> Result<()> {
    let a_rows = tx
        .query(
            "SELECT assertion_id, kind, statement, untrusted, private, valid_from, valid_to,
                    created_at, access_count, feedback_weight
             FROM public.memory_assertions
             WHERE tenant_id = $1
               AND embedding IS NOT NULL
               AND ($2::text IS NULL OR model = $2)
               AND ($3::text IS NULL OR kind = $3)
             ORDER BY embedding <=> $4::text::vector ASC
             LIMIT $5",
            &[&tenant_id, &model, &kind_filter, &vec_lit, &limit],
        )
        .await?;
    for (rank, r) in a_rows.into_iter().enumerate() {
        let id: String = r.get(0);
        out.entry(id.clone()).and_modify(|c| c.vec_rank = Some(c.vec_rank.map_or(rank, |prev| prev.min(rank)))).or_insert(Candidate {
            id,
            record_kind: r.get(1),
            text: r.get(2),
            label: TrustLabel { untrusted: r.get(3), private: r.get(4) },
            valid_from: r.get(5),
            valid_to: r.get(6),
            created_at: r.get(7),
            access_count: r.get(8),
            feedback_weight: r.get(9),
            fts_rank: None,
            vec_rank: Some(rank),
            exact_match: false,
            temporal_match: false,
        });
    }

    if kind_filter.is_none() || kind_filter == Some("source") {
        let s_rows = tx
            .query(
                "SELECT source_id, text, untrusted, private, occurred_at, created_at
                 FROM public.memory_sources
                 WHERE tenant_id = $1
                   AND embedding IS NOT NULL
                   AND ($2::text IS NULL OR model = $2)
                 ORDER BY embedding <=> $3::text::vector ASC
                 LIMIT $4",
                &[&tenant_id, &model, &vec_lit, &limit],
            )
            .await?;
        for (rank, r) in s_rows.into_iter().enumerate() {
            let id: String = r.get(0);
            out.entry(id.clone()).and_modify(|c| c.vec_rank = Some(c.vec_rank.map_or(rank, |prev| prev.min(rank)))).or_insert(Candidate {
                id,
                record_kind: "source".into(),
                text: r.get(1),
                label: TrustLabel { untrusted: r.get(2), private: r.get(3) },
                valid_from: Some(r.get(4)),
                valid_to: None,
                created_at: r.get(5),
                access_count: 0,
                feedback_weight: 1.0,
                fts_rank: None,
                vec_rank: Some(rank),
                exact_match: false,
                temporal_match: false,
            });
        }
    }
    Ok(())
}

async fn collect_temporal(tx: &Transaction<'_>, tenant_id: &str, win_start: DateTime<Utc>, win_end: DateTime<Utc>, limit: i64, out: &mut HashMap<String, Candidate>) -> Result<()> {
    let a_rows = tx
        .query(
            "SELECT assertion_id, kind, statement, untrusted, private, valid_from, valid_to,
                    created_at, access_count, feedback_weight
             FROM public.memory_assertions
             WHERE tenant_id = $1
               AND (
                   (valid_from IS NOT NULL AND valid_from < $3 AND (valid_to IS NULL OR valid_to >= $2))
                   OR (valid_from IS NULL AND created_at >= $2 AND created_at < $3)
               )
             ORDER BY COALESCE(valid_from, created_at) DESC
             LIMIT $4",
            &[&tenant_id, &win_start, &win_end, &limit],
        )
        .await?;
    for r in a_rows {
        let id: String = r.get(0);
        out.entry(id.clone()).and_modify(|c| c.temporal_match = true).or_insert(Candidate {
            id,
            record_kind: r.get(1),
            text: r.get(2),
            label: TrustLabel { untrusted: r.get(3), private: r.get(4) },
            valid_from: r.get(5),
            valid_to: r.get(6),
            created_at: r.get(7),
            access_count: r.get(8),
            feedback_weight: r.get(9),
            fts_rank: None,
            vec_rank: None,
            exact_match: false,
            temporal_match: true,
        });
    }
    Ok(())
}

async fn fetch_citations(tx: &Transaction<'_>, tenant_id: &str, assertion_id: &str) -> Result<Vec<String>> {
    let rows = tx
        .query(
            "SELECT DISTINCT source_id FROM public.memory_evidence
             WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $2
             ORDER BY source_id",
            &[&tenant_id, &assertion_id],
        )
        .await?;
    Ok(rows.into_iter().map(|r| r.get(0)).collect())
}

async fn collect_recent(tx: &Transaction<'_>, tenant_id: &str, kind_filter: Option<&str>, limit: i64, out: &mut HashMap<String, Candidate>) -> Result<()> {
    let rows = tx
        .query(
            "SELECT assertion_id, kind, statement, untrusted, private, valid_from, valid_to,
                    created_at, access_count, feedback_weight
             FROM public.memory_assertions
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR kind = $2)
             ORDER BY feedback_weight DESC, created_at DESC
             LIMIT $3",
            &[&tenant_id, &kind_filter, &limit],
        )
        .await?;
    for (rank, r) in rows.into_iter().enumerate() {
        let id: String = r.get(0);
        out.entry(id.clone()).or_insert(Candidate {
            id,
            record_kind: r.get(1),
            text: r.get(2),
            label: TrustLabel { untrusted: r.get(3), private: r.get(4) },
            valid_from: r.get(5),
            valid_to: r.get(6),
            created_at: r.get(7),
            access_count: r.get(8),
            feedback_weight: r.get(9),
            fts_rank: Some(rank),
            vec_rank: None,
            exact_match: false,
            temporal_match: false,
        });
    }
    Ok(())
}
