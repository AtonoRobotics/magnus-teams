//! Bounded 1-2 hop RLS + taint-filtered SQL graph expansion over `memory_edges`.

use crate::memory::{ReadScope, SourceRef, TaintLabel};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashSet};
use tokio_postgres::Transaction;

/// Maximum SQL graph expansion depth (hops).
pub const MAX_GRAPH_HOPS: usize = 2;
/// Maximum total edges returned by a single graph expansion.
pub const MAX_GRAPH_EDGES: usize = 24;
/// Maximum seed entities used to start graph expansion.
pub const MAX_SEED_ENTITIES: usize = 16;
/// Maximum word-window width for multi-word entity name matching (e.g. "Port Congestion").
pub const MAX_ENTITY_NGRAM_WORDS: usize = 3;

/// Single edge returned by bounded graph expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEdgeHit {
    pub edge_id: String,
    pub src_id: String,
    pub dst_id: String,
    pub src_name: String,
    pub dst_name: String,
    pub rel_type: String,
    pub label: TaintLabel,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub hop: usize,
}

/// Build lowercased single-token and 2..=`MAX_ENTITY_NGRAM_WORDS` phrase candidates
/// so multi-word entity names like `"Port Congestion"` match indexed `lower(src_name)`.
pub fn entity_lookup_candidates(tokens: &[String]) -> Vec<String> {
    let lower: Vec<String> = tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
    let mut candidates = BTreeSet::new();
    for width in 1..=MAX_ENTITY_NGRAM_WORDS {
        if lower.len() < width {
            break;
        }
        for win in lower.windows(width) {
            candidates.insert(win.join(" "));
        }
    }
    candidates.into_iter().collect()
}

/// Find seed `entity_id`s inside an already RLS-scoped reader transaction by
/// matching exact `external_id` or indexed `lower(src_name)`/`lower(dst_name)`
/// (including multi-word n-grams), falling back to exact `(source_id, source_rev)`
/// citations only when no direct entity matched.
pub async fn find_seed_entities(tx: &Transaction<'_>, scope: ReadScope<'_>, exact_tokens: &[String], cited_sources: &[SourceRef]) -> Result<Vec<String>> {
    let mut seeds = BTreeSet::new();
    let max_seeds_sql = MAX_SEED_ENTITIES as i64;
    let lookup_terms = entity_lookup_candidates(exact_tokens);

    if !lookup_terms.is_empty() {
        let rows = tx
            .query(
                "SELECT DISTINCT entity_id FROM (
                     SELECT e.src_id AS entity_id
                     FROM public.memory_edges e
                     LEFT JOIN public.memory_entities es ON es.tenant_id = e.tenant_id AND es.entity_id = e.src_id
                     WHERE e.tenant_id = $1
                       AND ($2 OR NOT e.untrusted)
                       AND (lower(e.src_name) = ANY($3) OR lower(COALESCE(es.external_id, '')) = ANY($3))
                     UNION
                     SELECT e.dst_id AS entity_id
                     FROM public.memory_edges e
                     LEFT JOIN public.memory_entities ed ON ed.tenant_id = e.tenant_id AND ed.entity_id = e.dst_id
                     WHERE e.tenant_id = $1
                       AND ($2 OR NOT e.untrusted)
                       AND (lower(e.dst_name) = ANY($3) OR lower(COALESCE(ed.external_id, '')) = ANY($3))
                 ) matched
                 LIMIT $4",
                &[&scope.tenant_id, &scope.allow_untrusted, &lookup_terms, &max_seeds_sql],
            )
            .await
            .context("query entity seeds by term")?;
        for row in rows {
            seeds.insert(row.get::<_, String>(0));
        }
    }

    if seeds.is_empty() && !cited_sources.is_empty() {
        let src_ids: Vec<String> = cited_sources.iter().map(|s| s.source_id.clone()).collect();
        let src_revs: Vec<i64> = cited_sources.iter().map(|s| s.rev).collect();
        let rows = tx
            .query(
                "SELECT DISTINCT e.src_id, e.dst_id
                 FROM unnest($2::text[], $3::bigint[]) AS c(source_id, source_rev)
                 JOIN public.memory_evidence ev
                   ON ev.tenant_id = $1
                  AND ev.target_kind = 'edge'
                  AND ev.source_id = c.source_id
                  AND ev.source_rev = c.source_rev
                 JOIN public.memory_edges e
                   ON e.tenant_id = ev.tenant_id
                  AND e.edge_id = ev.target_id
                 WHERE ($4 OR NOT e.untrusted)
                 LIMIT $5",
                &[&scope.tenant_id, &src_ids, &src_revs, &scope.allow_untrusted, &max_seeds_sql],
            )
            .await
            .context("query entity seeds by cited source revisions")?;
        for row in rows {
            seeds.insert(row.get::<_, String>(0));
            seeds.insert(row.get::<_, String>(1));
        }
    }

    Ok(seeds.into_iter().take(MAX_SEED_ENTITIES).collect())
}

/// Expand 1..=`max_hops` over RLS-filtered `memory_edges`, enforcing
/// `allow_untrusted` and temporal validity on every hop. Rejects `max_hops > MAX_GRAPH_HOPS`.
pub async fn expand_hops(tx: &Transaction<'_>, scope: ReadScope<'_>, seed_entity_ids: &[String], max_hops: usize) -> Result<Vec<GraphEdgeHit>> {
    anyhow::ensure!(max_hops <= MAX_GRAPH_HOPS, "requested max_graph_hops ({max_hops}) exceeds MAX_GRAPH_HOPS ({MAX_GRAPH_HOPS})");
    if seed_entity_ids.is_empty() || max_hops == 0 {
        return Ok(Vec::new());
    }

    let (range_start, range_end) = match scope.time_range {
        Some((s, e)) => (Some(s), Some(e)),
        None => (None, None),
    };

    let mut frontier: Vec<String> = seed_entity_ids.to_vec();
    let mut visited_entities: HashSet<String> = frontier.iter().cloned().collect();
    let mut visited_edges: HashSet<String> = HashSet::new();
    let mut out: Vec<GraphEdgeHit> = Vec::new();

    for hop in 1..=max_hops {
        if frontier.is_empty() || out.len() >= MAX_GRAPH_EDGES {
            break;
        }
        let remaining = (MAX_GRAPH_EDGES - out.len()) as i64;
        let rows = tx
            .query(
                "SELECT edge_id, src_id, dst_id, src_name, dst_name, rel_type,
                        untrusted, private, valid_from, valid_to
                 FROM public.memory_edges
                 WHERE tenant_id = $1
                   AND (src_id = ANY($2) OR dst_id = ANY($2))
                   AND ($3 OR NOT untrusted)
                   AND (
                       ($4::timestamptz IS NULL AND (valid_to IS NULL OR valid_to > now()))
                       OR ($4::timestamptz IS NOT NULL AND valid_from <= $5::timestamptz AND (valid_to IS NULL OR valid_to >= $4::timestamptz))
                   )
                 ORDER BY valid_from DESC, edge_id ASC
                 LIMIT $6",
                &[&scope.tenant_id, &frontier, &scope.allow_untrusted, &range_start, &range_end, &remaining],
            )
            .await
            .context("expand memory_edges hop")?;

        let mut next_frontier = Vec::new();
        for row in rows {
            let edge_id: String = row.get(0);
            if !visited_edges.insert(edge_id.clone()) {
                continue;
            }
            let src_id: String = row.get(1);
            let dst_id: String = row.get(2);
            if visited_entities.insert(src_id.clone()) {
                next_frontier.push(src_id.clone());
            }
            if visited_entities.insert(dst_id.clone()) {
                next_frontier.push(dst_id.clone());
            }
            out.push(GraphEdgeHit {
                edge_id,
                src_id,
                dst_id,
                src_name: row.get(3),
                dst_name: row.get(4),
                rel_type: row.get(5),
                label: TaintLabel { untrusted: row.get(6), private: row.get(7) },
                valid_from: row.get(8),
                valid_to: row.get(9),
                hop,
            });
            if out.len() >= MAX_GRAPH_EDGES {
                break;
            }
        }
        frontier = next_frontier;
    }

    Ok(out)
}
