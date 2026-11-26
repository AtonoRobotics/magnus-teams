//! Pure read-only 0-LLM hybrid retrieval engine: FTS (`tsvector`) + `pgvector`
//! HNSW iterative scan (re-ordered by exact distance) fused via RRF, read-time
//! visibility-scoped supersession with private-influence taint tracking,
//! unenriched `memory_sources` fallback, and bounded 1-2 hop graph expansion.

use crate::memory::graph::{expand_hops, find_seed_entities, GraphEdgeHit, MAX_GRAPH_HOPS};
use crate::memory::store::MemoryStore;
use crate::memory::{format_halfvec_literal, AssertionKind, ReadScope, SourceRef, TaintLabel};
use crate::memory_index::{hybrid_score, RankedCandidate};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};
use tokio_postgres::{Row, Transaction};

/// Default maximum number of assertion/source hits returned.
pub const DEFAULT_RETRIEVE_LIMIT: usize = 12;
/// Default approximate token budget for packed context.
pub const DEFAULT_TOKEN_BUDGET: usize = 2000;
/// Conservative character-to-token ratio for budget packing.
pub const CHARS_PER_TOKEN: usize = 4;
/// Per-branch SQL candidate pool size before RRF fusion.
pub const BRANCH_CANDIDATE_LIMIT: usize = 40;
/// Multiplier for exact ID / code / phrase matches.
pub const EXACT_MATCH_BOOST: f64 = 1.5;
/// Minimum character length for query tokens/lexemes.
pub const MIN_QUERY_TOKEN_CHARS: usize = 2;
/// Words longer than this match by their stem: their first two thirds, never
/// shorter than this ("allergy" finds "allergic", "schedules" "scheduled").
pub const STEM_MIN_CHARS: usize = 4;
/// Shortest word matched as a prefix rather than whole; shorter ones ("of",
/// "is") would match nearly everything.
pub const PREFIX_MIN_CHARS: usize = 3;
/// Seconds per calendar day for recency decay calculation.
pub const SECONDS_PER_DAY: f64 = 86_400.0;
/// Formatting separator overhead (bytes) per serialized graph edge (`" -> "`).
pub const EDGE_FORMAT_OVERHEAD_BYTES: usize = 4;

/// Input query to the unified memory retriever.
#[derive(Clone, Debug)]
pub struct RetrieveRequest {
    pub tenant_id: String,
    pub principals: Vec<String>,
    pub query: String,
    pub query_embedding: Option<Vec<f32>>,
    /// Optional structured `[start, end]` temporal window. When `None`, only
    /// currently active rows (`valid_to IS NULL OR valid_to > now()`) match.
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// When `false`, strictly excludes `untrusted` rows and edges at every stage.
    pub allow_untrusted: bool,
    pub limit: usize,
    pub token_budget: usize,
    pub max_graph_hops: usize,
}

impl RetrieveRequest {
    pub fn new(tenant_id: impl Into<String>, principals: Vec<String>, query: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            principals,
            query: query.into(),
            query_embedding: None,
            time_range: None,
            allow_untrusted: false,
            limit: DEFAULT_RETRIEVE_LIMIT,
            token_budget: DEFAULT_TOKEN_BUDGET,
            max_graph_hops: MAX_GRAPH_HOPS,
        }
    }
}

/// Origin table of a retrieved hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitOrigin {
    Assertion,
    UnenrichedSource,
}

/// Typed kind of a retrieved hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Assertion(AssertionKind),
    Source,
}

/// Typed key distinguishing assertion candidates from unenriched source candidates.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CandidateKey {
    Assertion(String),
    Source(SourceRef),
}

/// Ranked memory hit returned to the agent with provenance and taint label.
#[derive(Clone, Debug)]
pub struct RetrieveHit {
    pub id: String,
    pub origin: HitOrigin,
    pub kind: HitKind,
    pub text: String,
    pub label: TaintLabel,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub score: f64,
    pub citations: Vec<SourceRef>,
}

/// Unified retrieval response with token-packed hits, graph edges, joined taint
/// label, and deduplicated source citations.
#[derive(Clone, Debug)]
pub struct RetrieveResponse {
    pub hits: Vec<RetrieveHit>,
    pub edges: Vec<GraphEdgeHit>,
    pub label: TaintLabel,
    pub sources: Vec<SourceRef>,
}

struct CandidateMeta {
    id: String,
    origin: HitOrigin,
    kind: HitKind,
    text: String,
    label: TaintLabel,
    valid_from: DateTime<Utc>,
    valid_to: Option<DateTime<Utc>>,
    access_count: u32,
    fts_rank: Option<usize>,
    vec_rank: Option<usize>,
}

/// Split query preserving hyphens/underscores for exact ID and multi-word entity matching.
fn exact_match_tokens(query: &str) -> Vec<String> {
    query.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').map(str::trim).filter(|w| w.chars().count() >= MIN_QUERY_TOKEN_CHARS).map(str::to_string).collect()
}

/// Split query into lowercased `to_tsquery('simple', ...)` terms: a long word
/// by its stem and a shorter one whole, each matched as a prefix, except the
/// shortest, matched whole.
fn fts_lexemes(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= MIN_QUERY_TOKEN_CHARS)
        .map(|w| {
            let n = w.chars().count();
            if n < PREFIX_MIN_CHARS {
                return w.to_string();
            }
            let keep = if n > STEM_MIN_CHARS { (n * 2 / 3).max(STEM_MIN_CHARS) } else { n };
            format!("{}:*", w.chars().take(keep).collect::<String>())
        })
        .collect()
}

/// Whether `query` has any word to search for.
pub fn has_terms(query: &str) -> bool {
    !fts_lexemes(query).is_empty()
}

fn valid_at_sql(alias: &str, start_param: &str, end_param: &str) -> String {
    format!(
        "(({start_param}::timestamptz IS NULL AND ({alias}.valid_to IS NULL OR {alias}.valid_to > now())) \
          OR ({start_param}::timestamptz IS NOT NULL AND {alias}.valid_from <= {end_param}::timestamptz \
              AND ({alias}.valid_to IS NULL OR {alias}.valid_to >= {start_param}::timestamptz)))"
    )
}

/// SQL predicate requiring `alias` to cover the entire active or requested
/// temporal window (not merely overlap part of it).
fn covers_window_sql(alias: &str, start_param: &str, end_param: &str) -> String {
    format!(
        "(({start_param}::timestamptz IS NULL AND {alias}.valid_from <= now() AND ({alias}.valid_to IS NULL OR {alias}.valid_to > now())) \
          OR ({start_param}::timestamptz IS NOT NULL AND {alias}.valid_from <= {start_param}::timestamptz \
              AND ({alias}.valid_to IS NULL OR {alias}.valid_to >= {end_param}::timestamptz)))"
    )
}

/// SQL expression returning:
/// - `superseded`: whether any visible, active, trust-eligible newer assertion covers the whole window.
/// - `superseded_by_private`: whether any such superseding assertion is `private`.
fn supersession_lateral_sql(alias: &str, allow_param: &str, start_param: &str, end_param: &str) -> String {
    let newer_covers = covers_window_sql("newer", start_param, end_param);
    format!(
        "LEFT JOIN LATERAL ( \
             SELECT (count(*) > 0) AS is_superseded, COALESCE(bool_or(newer.private), false) AS by_private \
             FROM public.memory_supersessions s \
             JOIN public.memory_assertions newer \
               ON newer.tenant_id = s.tenant_id \
              AND newer.assertion_id = s.new_assertion_id \
             WHERE s.tenant_id = {alias}.tenant_id \
               AND s.old_assertion_id = {alias}.assertion_id \
               AND NOT newer.archived \
               AND ({allow_param} OR NOT newer.untrusted) \
               AND ({alias}.untrusted OR NOT newer.untrusted) \
               AND {newer_covers} \
         ) sup ON true"
    )
}

fn meta_from_assertion_row(row: &Row, fts_rank: Option<usize>, vec_rank: Option<usize>) -> Result<CandidateMeta> {
    let raw_kind: String = row.get(1);
    Ok(CandidateMeta {
        id: row.get(0),
        origin: HitOrigin::Assertion,
        kind: HitKind::Assertion(AssertionKind::parse(&raw_kind)?),
        text: row.get(2),
        label: TaintLabel { untrusted: row.get(3), private: row.get(4) },
        valid_from: row.get(5),
        valid_to: row.get(6),
        access_count: row.get::<_, i32>(7).max(0) as u32,
        fts_rank,
        vec_rank,
    })
}

async fn query_fts_assertions(
    tx: &Transaction<'_>,
    scope: ReadScope<'_>,
    lexemes: &[String],
    candidates: &mut HashMap<CandidateKey, CandidateMeta>,
    suppressed_by_private: &mut bool,
) -> Result<()> {
    if lexemes.is_empty() {
        return Ok(());
    }
    let (range_start, range_end) = match scope.time_range {
        Some((s, e)) => (Some(s), Some(e)),
        None => (None, None),
    };
    let ts_expr = lexemes.join(" | ");
    let limit_sql = BRANCH_CANDIDATE_LIMIT as i64;
    let sql = format!(
        "SELECT a.assertion_id, a.kind, a.statement, a.untrusted, a.private,
                a.valid_from, a.valid_to, a.access_count,
                COALESCE(sup.is_superseded, false) AS is_superseded,
                COALESCE(sup.by_private, false) AS by_private
         FROM public.memory_assertions a
         {sup_join}
         WHERE a.tenant_id = $1
           AND NOT a.archived
           AND ($2 OR NOT a.untrusted)
           AND a.tsv @@ to_tsquery('simple', $3)
           AND {valid}
         ORDER BY ts_rank_cd(a.tsv, to_tsquery('simple', $3)) DESC, a.valid_from DESC
         LIMIT $6",
        sup_join = supersession_lateral_sql("a", "$2", "$4", "$5"),
        valid = valid_at_sql("a", "$4", "$5"),
    );
    let rows = tx.query(&sql, &[&scope.tenant_id, &scope.allow_untrusted, &ts_expr, &range_start, &range_end, &limit_sql]).await.context("FTS query on memory_assertions")?;

    let mut rank = 0usize;
    for row in rows {
        let is_superseded: bool = row.get(8);
        let by_private: bool = row.get(9);
        if is_superseded {
            if by_private {
                *suppressed_by_private = true;
            }
            continue;
        }
        let meta = meta_from_assertion_row(&row, Some(rank), None)?;
        candidates.insert(CandidateKey::Assertion(meta.id.clone()), meta);
        rank += 1;
    }
    Ok(())
}

async fn query_vec_assertions(
    tx: &Transaction<'_>,
    scope: ReadScope<'_>,
    vec_lit: &str,
    candidates: &mut HashMap<CandidateKey, CandidateMeta>,
    suppressed_by_private: &mut bool,
) -> Result<()> {
    let (range_start, range_end) = match scope.time_range {
        Some((s, e)) => (Some(s), Some(e)),
        None => (None, None),
    };
    let limit_sql = BRANCH_CANDIDATE_LIMIT as i64;
    // Inner query leverages pgvector HNSW iterative_scan; outer query re-sorts
    // by exact cosine distance so `vec_rank` is strictly exact under relaxed_order.
    let sql = format!(
        "SELECT assertion_id, kind, statement, untrusted, private,
                valid_from, valid_to, access_count, is_superseded, by_private
         FROM (
             SELECT a.assertion_id, a.kind, a.statement, a.untrusted, a.private,
                    a.valid_from, a.valid_to, a.access_count,
                    COALESCE(sup.is_superseded, false) AS is_superseded,
                    COALESCE(sup.by_private, false) AS by_private,
                    (a.embedding <=> $3::text::halfvec) AS dist
             FROM public.memory_assertions a
             {sup_join}
             WHERE a.tenant_id = $1
               AND NOT a.archived
               AND a.embedding IS NOT NULL
               AND ($2 OR NOT a.untrusted)
               AND {valid}
             ORDER BY a.embedding <=> $3::text::halfvec ASC
             LIMIT $6
         ) ranked
         ORDER BY dist ASC",
        sup_join = supersession_lateral_sql("a", "$2", "$4", "$5"),
        valid = valid_at_sql("a", "$4", "$5"),
    );
    let rows =
        tx.query(&sql, &[&scope.tenant_id, &scope.allow_untrusted, &vec_lit, &range_start, &range_end, &limit_sql]).await.context("HNSW vector query on memory_assertions")?;

    let mut rank = 0usize;
    for row in rows {
        let is_superseded: bool = row.get(8);
        let by_private: bool = row.get(9);
        if is_superseded {
            if by_private {
                *suppressed_by_private = true;
            }
            continue;
        }
        let id: String = row.get(0);
        let key = CandidateKey::Assertion(id);
        if let Some(existing) = candidates.get_mut(&key) {
            existing.vec_rank = Some(rank);
        } else {
            candidates.insert(key, meta_from_assertion_row(&row, None, Some(rank))?);
        }
        rank += 1;
    }
    Ok(())
}

async fn query_unenriched_sources(
    tx: &Transaction<'_>,
    scope: ReadScope<'_>,
    lexemes: &[String],
    candidates: &mut HashMap<CandidateKey, CandidateMeta>,
    citations: &mut HashMap<CandidateKey, Vec<SourceRef>>,
) -> Result<()> {
    if lexemes.is_empty() {
        return Ok(());
    }
    let (range_start, range_end) = match scope.time_range {
        Some((s, e)) => (Some(s), Some(e)),
        None => (None, None),
    };
    let ts_expr = lexemes.join(" | ");
    let limit_sql = BRANCH_CANDIDATE_LIMIT as i64;
    // Joins memory_assertions (RLS-filtered) so a reader who can see a source
    // but none of its narrower-ACL derived assertions still gets the fallback.
    let rows = tx
        .query(
            "SELECT s.source_id, s.rev, s.text, s.untrusted, s.private, s.occurred_at
             FROM public.memory_sources s
             WHERE s.tenant_id = $1
               AND ($2 OR NOT s.untrusted)
               AND s.tsv @@ to_tsquery('simple', $3)
               AND ($4::timestamptz IS NULL OR (s.occurred_at >= $4::timestamptz AND s.occurred_at <= $5::timestamptz))
               AND NOT EXISTS (
                   SELECT 1
                   FROM public.memory_evidence ev
                   JOIN public.memory_assertions a
                     ON a.tenant_id = ev.tenant_id
                    AND a.assertion_id = ev.target_id
                   WHERE ev.tenant_id = s.tenant_id
                     AND ev.source_id = s.source_id
                     AND ev.source_rev = s.rev
                     AND ev.target_kind = 'assertion'
               )
             ORDER BY ts_rank_cd(s.tsv, to_tsquery('simple', $3)) DESC, s.occurred_at DESC
             LIMIT $6",
            &[&scope.tenant_id, &scope.allow_untrusted, &ts_expr, &range_start, &range_end, &limit_sql],
        )
        .await
        .context("FTS fallback query on unenriched memory_sources")?;

    for (rank, row) in rows.into_iter().enumerate() {
        let sref = SourceRef { source_id: row.get(0), rev: row.get(1) };
        let key = CandidateKey::Source(sref.clone());
        let id = format!("{}@{}", sref.source_id, sref.rev);
        citations.insert(key.clone(), vec![sref]);
        candidates.insert(
            key,
            CandidateMeta {
                id,
                origin: HitOrigin::UnenrichedSource,
                kind: HitKind::Source,
                text: row.get(2),
                label: TaintLabel { untrusted: row.get(3), private: row.get(4) },
                valid_from: row.get(5),
                valid_to: None,
                access_count: 0,
                fts_rank: Some(rank),
                vec_rank: None,
            },
        );
    }
    Ok(())
}

async fn load_assertion_citations(
    tx: &Transaction<'_>,
    tenant_id: &str,
    candidates: &HashMap<CandidateKey, CandidateMeta>,
    citations: &mut HashMap<CandidateKey, Vec<SourceRef>>,
) -> Result<()> {
    let assertion_ids: Vec<String> = candidates
        .keys()
        .filter_map(|k| match k {
            CandidateKey::Assertion(id) => Some(id.clone()),
            CandidateKey::Source(_) => None,
        })
        .collect();
    if assertion_ids.is_empty() {
        return Ok(());
    }
    let ev_rows = tx
        .query(
            "SELECT target_id, source_id, source_rev
             FROM public.memory_evidence
             WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = ANY($2)",
            &[&tenant_id, &assertion_ids],
        )
        .await
        .context("load assertion citations")?;
    for row in ev_rows {
        let key = CandidateKey::Assertion(row.get(0));
        let sref = SourceRef { source_id: row.get(1), rev: row.get(2) };
        citations.entry(key).or_default().push(sref);
    }
    Ok(())
}

fn rank_and_pack(
    req: &RetrieveRequest,
    exact_tokens: &[String],
    candidates: HashMap<CandidateKey, CandidateMeta>,
    mut citations: HashMap<CandidateKey, Vec<SourceRef>>,
    edges: Vec<GraphEdgeHit>,
    suppressed_by_private: bool,
) -> RetrieveResponse {
    let now = Utc::now();
    let needle = req.query.trim().to_ascii_lowercase();

    let mut scored: Vec<RetrieveHit> = candidates
        .into_iter()
        .map(|(key, meta)| {
            let age_days = (now - meta.valid_from).num_seconds().max(0) as f64 / SECONDS_PER_DAY;
            let base = hybrid_score(&RankedCandidate {
                id: meta.id.clone(),
                fts_rank: meta.fts_rank,
                vec_rank: meta.vec_rank,
                untrusted: meta.label.untrusted,
                age_days,
                access_count: meta.access_count,
            });
            let lower_text = meta.text.to_ascii_lowercase();
            let exact = !needle.is_empty()
                && (lower_text.contains(&needle) || exact_tokens.iter().any(|t| t.chars().any(|c| c.is_ascii_digit()) && lower_text.contains(&t.to_ascii_lowercase())));
            let score = if exact { base * EXACT_MATCH_BOOST } else { base };
            let hit_citations = citations.remove(&key).unwrap_or_default();
            RetrieveHit {
                id: meta.id,
                origin: meta.origin,
                kind: meta.kind,
                text: meta.text,
                label: meta.label,
                valid_from: meta.valid_from,
                valid_to: meta.valid_to,
                score,
                citations: hit_citations,
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| b.valid_from.cmp(&a.valid_from)));

    let char_budget = req.token_budget.saturating_mul(CHARS_PER_TOKEN);
    let mut used_chars = 0usize;
    let mut packed_hits = Vec::new();
    let mut joined_label = TaintLabel { untrusted: false, private: suppressed_by_private };
    let mut all_sources = BTreeSet::new();

    for hit in scored.into_iter().take(req.limit) {
        let cost = hit.text.len().max(1);
        if !packed_hits.is_empty() && used_chars.saturating_add(cost) > char_budget {
            break;
        }
        used_chars = used_chars.saturating_add(cost);
        joined_label = joined_label.join(hit.label);
        for c in &hit.citations {
            all_sources.insert(c.clone());
        }
        packed_hits.push(hit);
    }

    let mut packed_edges = Vec::new();
    for edge in edges {
        let cost = edge.src_name.len() + edge.rel_type.len() + edge.dst_name.len() + EDGE_FORMAT_OVERHEAD_BYTES;
        if used_chars.saturating_add(cost) > char_budget {
            break;
        }
        used_chars = used_chars.saturating_add(cost);
        joined_label = joined_label.join(edge.label);
        packed_edges.push(edge);
    }

    RetrieveResponse { hits: packed_hits, edges: packed_edges, label: joined_label, sources: all_sources.into_iter().collect() }
}

/// Execute unified read-only hybrid FTS + `pgvector` HNSW + read-time
/// visibility-scoped supersession + bounded 1-2 hop graph expansion (0 LLM calls).
pub async fn retrieve(store: &mut MemoryStore, req: &RetrieveRequest) -> Result<RetrieveResponse> {
    anyhow::ensure!(req.max_graph_hops <= MAX_GRAPH_HOPS, "requested max_graph_hops ({}) exceeds MAX_GRAPH_HOPS ({MAX_GRAPH_HOPS})", req.max_graph_hops);
    let emb_lit = match &req.query_embedding {
        Some(v) => Some(format_halfvec_literal(v, store.embedding_dim())?),
        None => None,
    };
    let lexemes = fts_lexemes(&req.query);
    let exact_tokens = exact_match_tokens(&req.query);
    let scope = ReadScope { tenant_id: &req.tenant_id, allow_untrusted: req.allow_untrusted, time_range: req.time_range };

    let mut candidates: HashMap<CandidateKey, CandidateMeta> = HashMap::new();
    let mut citations: HashMap<CandidateKey, Vec<SourceRef>> = HashMap::new();
    let mut suppressed_by_private = false;

    let tx = store.begin_reader_tx(&req.tenant_id, &req.principals).await?;
    query_fts_assertions(&tx, scope, &lexemes, &mut candidates, &mut suppressed_by_private).await?;
    if let Some(ref vec_lit) = emb_lit {
        query_vec_assertions(&tx, scope, vec_lit, &mut candidates, &mut suppressed_by_private).await?;
    }
    query_unenriched_sources(&tx, scope, &lexemes, &mut candidates, &mut citations).await?;
    load_assertion_citations(&tx, &req.tenant_id, &candidates, &mut citations).await?;

    let cited_sources: Vec<SourceRef> = citations.values().flat_map(|v| v.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
    let seeds = find_seed_entities(&tx, scope, &exact_tokens, &cited_sources).await?;
    let edges = expand_hops(&tx, scope, &seeds, req.max_graph_hops).await?;
    tx.commit().await?;

    Ok(rank_and_pack(req, &exact_tokens, candidates, citations, edges, suppressed_by_private))
}

/// The reader's most recent sources, newest first: what "everything you
/// remember" answers when a query has no word to search for.
pub async fn recent_sources(store: &mut MemoryStore, req: &RetrieveRequest) -> Result<RetrieveResponse> {
    let limit = req.limit as i64;
    let tx = store.begin_reader_tx(&req.tenant_id, &req.principals).await?;
    let rows = tx
        .query(
            "SELECT source_id, rev, text, untrusted, private, occurred_at
             FROM public.memory_sources
             WHERE tenant_id = $1 AND ($2 OR NOT untrusted)
             ORDER BY occurred_at DESC, source_id
             LIMIT $3",
            &[&req.tenant_id, &req.allow_untrusted, &limit],
        )
        .await
        .context("list the reader's recent memory sources")?;
    tx.commit().await?;
    let hits: Vec<RetrieveHit> = rows
        .iter()
        .map(|row| {
            let sref = SourceRef { source_id: row.get(0), rev: row.get(1) };
            RetrieveHit {
                id: format!("{}@{}", sref.source_id, sref.rev),
                origin: HitOrigin::UnenrichedSource,
                kind: HitKind::Source,
                text: row.get(2),
                label: TaintLabel { untrusted: row.get(3), private: row.get(4) },
                valid_from: row.get(5),
                valid_to: None,
                score: 0.0,
                citations: vec![sref],
            }
        })
        .collect();
    let label =
        hits.iter().fold(TaintLabel { untrusted: false, private: false }, |l, h| TaintLabel { untrusted: l.untrusted || h.label.untrusted, private: l.private || h.label.private });
    let sources = hits.iter().flat_map(|h| h.citations.iter().cloned()).collect();
    Ok(RetrieveResponse { hits, edges: Vec::new(), label, sources })
}
