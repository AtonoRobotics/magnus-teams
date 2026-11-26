//! Bounded RLS-filtered SQL graph expansion and read-time visibility-scoped
//! supersession / conflict resolution.
//!
//! All queries in this module run inside an active transaction that has already
//! executed `SET LOCAL ROLE {role}_reader` and configured `magnus.tenant` and
//! `magnus.principals`. Every hop, supersession check, and conflict pair is
//! therefore strictly filtered by Postgres RLS.

use crate::memory::{ConflictPair, GraphFact, MAX_EDGES_PER_SEED, MAX_GRAPH_HOPS};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use tokio_postgres::Transaction;

/// Expand a bounded 1..=`hops` neighborhood around `seed_ids` over `memory_edges`,
/// resolving human-readable node labels from `memory_entities` and `memory_assertions`.
pub(crate) async fn expand_neighborhood(tx: &Transaction<'_>, tenant_id: &str, seed_ids: &[String], hops: usize) -> Result<Vec<GraphFact>> {
    if seed_ids.is_empty() || hops == 0 {
        return Ok(Vec::new());
    }
    let max_hops = hops.min(MAX_GRAPH_HOPS);
    let mut frontier: Vec<String> = seed_ids.to_vec();
    let mut visited_nodes: HashSet<String> = seed_ids.iter().cloned().collect();
    let mut seen_edges: HashSet<String> = HashSet::new();
    let mut raw_edges: Vec<(String, String, String, String, bool)> = Vec::new();

    for _ in 0..max_hops {
        if frontier.is_empty() {
            break;
        }
        let limit = (frontier.len() * MAX_EDGES_PER_SEED).min(100) as i64;
        let rows = tx
            .query(
                "SELECT edge_id, src_id, dst_id, rel_type, untrusted
                 FROM public.memory_edges
                 WHERE tenant_id = $1
                   AND rel_type NOT IN ('supersedes', 'contradicts')
                   AND (src_id = ANY($2) OR dst_id = ANY($2))
                 ORDER BY created_at DESC
                 LIMIT $3",
                &[&tenant_id, &frontier, &limit],
            )
            .await
            .context("expand memory_edges neighborhood")?;

        let mut next_frontier = Vec::new();
        for r in rows {
            let edge_id: String = r.get(0);
            if !seen_edges.insert(edge_id.clone()) {
                continue;
            }
            let src_id: String = r.get(1);
            let dst_id: String = r.get(2);
            let rel_type: String = r.get(3);
            let untrusted: bool = r.get(4);
            if visited_nodes.insert(src_id.clone()) {
                next_frontier.push(src_id.clone());
            }
            if visited_nodes.insert(dst_id.clone()) {
                next_frontier.push(dst_id.clone());
            }
            raw_edges.push((edge_id, src_id, dst_id, rel_type, untrusted));
        }
        frontier = next_frontier;
    }

    if raw_edges.is_empty() {
        return Ok(Vec::new());
    }

    let all_ids: Vec<String> = visited_nodes.into_iter().collect();
    let labels = resolve_node_labels(tx, tenant_id, &all_ids).await?;

    Ok(raw_edges
        .into_iter()
        .map(|(edge_id, src_id, dst_id, rel_type, untrusted)| GraphFact {
            edge_id,
            src_name: labels.get(&src_id).cloned().unwrap_or_else(|| src_id.clone()),
            dst_name: labels.get(&dst_id).cloned().unwrap_or_else(|| dst_id.clone()),
            src_id,
            dst_id,
            rel_type,
            untrusted,
        })
        .collect())
}

/// Resolve human-readable display names for entity or assertion node IDs under RLS.
async fn resolve_node_labels(tx: &Transaction<'_>, tenant_id: &str, ids: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::with_capacity(ids.len());
    let ent_rows = tx
        .query(
            "SELECT entity_id, canonical_name FROM public.memory_entities
             WHERE tenant_id = $1 AND entity_id = ANY($2)",
            &[&tenant_id, &ids],
        )
        .await?;
    for r in ent_rows {
        map.insert(r.get::<_, String>(0), r.get::<_, String>(1));
    }

    let assert_rows = tx
        .query(
            "SELECT assertion_id, statement FROM public.memory_assertions
             WHERE tenant_id = $1 AND assertion_id = ANY($2)",
            &[&tenant_id, &ids],
        )
        .await?;
    for r in assert_rows {
        map.entry(r.get::<_, String>(0)).or_insert_with(|| r.get::<_, String>(1));
    }
    Ok(map)
}

/// Given candidate assertion IDs visible to the current reader, return:
/// 1. `superseded_ids`: assertions superseded by a *visible* `supersedes` edge
///    whose replacement assertion is also visible and active.
/// 2. `conflicts`: unresolved `contradicts` pairs visible to the current reader.
///
/// Because this query runs under `{role}_reader` RLS, a private superseding
/// assertion hides the old shared assertion *only* for readers authorized to see
/// the private assertion, never for teammates.
pub(crate) async fn resolve_supersession_and_conflicts(tx: &Transaction<'_>, tenant_id: &str, candidate_ids: &[String]) -> Result<(HashSet<String>, Vec<ConflictPair>)> {
    if candidate_ids.is_empty() {
        return Ok((HashSet::new(), Vec::new()));
    }

    // An edge `src_id --supersedes--> dst_id` means `src_id` replaces `dst_id`.
    // Verify both the edge AND the replacing assertion (`src_id`) are visible to this reader.
    let sup_rows = tx
        .query(
            "SELECT DISTINCT e.dst_id
             FROM public.memory_edges e
             JOIN public.memory_assertions replacement
               ON replacement.tenant_id = e.tenant_id
              AND replacement.assertion_id = e.src_id
             WHERE e.tenant_id = $1
               AND e.rel_type = 'supersedes'
               AND e.dst_id = ANY($2)",
            &[&tenant_id, &candidate_ids],
        )
        .await
        .context("resolve visibility-scoped supersession")?;

    let superseded: HashSet<String> = sup_rows.into_iter().map(|r| r.get(0)).collect();

    let conflict_rows = tx
        .query(
            "SELECT e.edge_id, a.assertion_id, a.statement, b.assertion_id, b.statement
             FROM public.memory_edges e
             JOIN public.memory_assertions a
               ON a.tenant_id = e.tenant_id AND a.assertion_id = e.src_id
             JOIN public.memory_assertions b
               ON b.tenant_id = e.tenant_id AND b.assertion_id = e.dst_id
             WHERE e.tenant_id = $1
               AND e.rel_type = 'contradicts'
               AND (e.src_id = ANY($2) OR e.dst_id = ANY($2))
             ORDER BY e.created_at DESC
             LIMIT 20",
            &[&tenant_id, &candidate_ids],
        )
        .await
        .context("resolve visibility-scoped conflicts")?;

    let mut conflicts = Vec::new();
    for r in conflict_rows {
        let a_id: String = r.get(1);
        let b_id: String = r.get(3);
        if superseded.contains(&a_id) || superseded.contains(&b_id) {
            continue;
        }
        conflicts.push(ConflictPair { edge_id: r.get(0), assertion_a: a_id, statement_a: r.get(2), assertion_b: b_id, statement_b: r.get(4) });
    }

    Ok((superseded, conflicts))
}
