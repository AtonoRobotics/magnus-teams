//! Live kernel event follower and background enrichment/curator loop for the
//! unified Postgres memory engine.
//!
//! Replaces the old in-RAM personal memory scan and unscheduled curator stubs:
//! - `Kernel::follow_memory` projects every memory, playbook, knowledge, and
//!   revocation/erasure event into Postgres immediately so FTS recall works on
//!   the very next turn.
//! - `schedule` runs a lightweight background loop that drains durable
//!   `memory_enrichment_jobs` (embedding via `Kernel::embed` when configured,
//!   fenced against live source revisions) and runs the watermark-gated
//!   maintenance curator (`CURATOR_POLL_INTERVAL`, 0 LLM calls when idle).

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::log::Event;
use crate::memory::curator::CollisionResolution;
use crate::memory::enrich::EnrichmentPayload;
use crate::memory::{AssertionInput, SourceInput, TrustLabel, CURATOR_POLL_INTERVAL, DEFAULT_TENANT_ID};
use crate::spend::allowance::Owner;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Poll interval for pending enrichment jobs.
const ENRICH_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Maximum enrichment jobs processed per tick.
const ENRICH_BATCH_LIMIT: usize = 16;

/// Parse a pgvector literal `"[0.1,0.2,...]"` returned by `Kernel::embed` into `Vec<f32>`.
pub(crate) fn parse_vector_literal(lit: &str) -> Option<Vec<f32>> {
    let inner = lit.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for part in inner.split(',') {
        out.push(part.trim().parse::<f32>().ok()?);
    }
    (!out.is_empty()).then_some(out)
}

impl Kernel {
    /// Project emitted kernel events into the unified Postgres memory engine.
    /// Runs immediately after `Kernel::emit_all` commits to `public.events`.
    pub(crate) async fn follow_memory(&self, es: &[Event]) {
        for e in es {
            if let Err(err) = self.apply_memory_event(e).await {
                tracing::warn!(seq = e.seq, "unified memory follow skipped: {err:#}");
            }
        }
    }

    async fn apply_memory_event(&self, e: &Event) -> anyhow::Result<()> {
        match &e.ev {
            Ev::Memory { id, person, fact, topic, label, sources, .. } => {
                let text = match topic.as_deref().filter(|t| !t.trim().is_empty()) {
                    Some(t) => format!("{fact} ({t})"),
                    None => fact.clone(),
                };
                let readers = vec![vec![format!("person:{person}")]];
                let trust = TrustLabel { untrusted: label.untrusted, private: label.private };
                let ing = self
                    .memory
                    .ingest_source(&SourceInput {
                        tenant_id: DEFAULT_TENANT_ID.into(),
                        source_id: id.clone(),
                        rev: 1,
                        owner_id: Some(person.clone()),
                        readers,
                        label: trust,
                        kind: "fact".into(),
                        text: text.clone(),
                        metadata: json!({ "topic": topic, "sources": sources }),
                        occurred_at: e.at,
                    })
                    .await?;
                if !ing.deduplicated {
                    // Make the assertion immediately searchable via FTS; the
                    // background worker will attach embeddings once computed.
                    let payload = EnrichmentPayload {
                        source_embedding: None,
                        model: None,
                        assertions: vec![AssertionInput {
                            assertion_id: id.clone(),
                            kind: "fact".into(),
                            statement: text,
                            valid_from: Some(e.at),
                            valid_to: None,
                            confidence: 1.0,
                            embedding: None,
                            model: None,
                        }],
                        entities: Vec::new(),
                        edges: Vec::new(),
                    };
                    // Leave job pending so the async worker computes embeddings.
                    self.memory.commit_enrichment(DEFAULT_TENANT_ID, id, 1, None, &payload).await?;
                }
            }
            Ev::MemoryForgotten { id } => {
                self.memory.revoke_source(DEFAULT_TENANT_ID, id, None).await?;
            }
            Ev::MemorySuperseded(p) => {
                self.memory.record_collision_edge(DEFAULT_TENANT_ID, &p.new_id, &p.old_id, CollisionResolution::Supersede, &p.reason).await?;
            }
            Ev::MemoryArchived(p) => {
                let mut guard = self.memory.client().await?;
                if let Some(c) = guard.as_mut() {
                    c.execute(
                        "UPDATE public.memory_assertions SET archived = true, updated_at = now()
                         WHERE tenant_id = $1 AND assertion_id = $2",
                        &[&DEFAULT_TENANT_ID, &p.id],
                    )
                    .await?;
                }
            }
            Ev::Playbook { item, op, person, text, label, .. } => {
                if op == "retire" {
                    self.memory.revoke_source(DEFAULT_TENANT_ID, item, None).await?;
                } else if let Some(body) = text.as_deref().filter(|s| !s.trim().is_empty()) {
                    let rev = e.seq.max(1);
                    let readers = vec![vec![format!("person:{person}")]];
                    let trust = TrustLabel { untrusted: label.untrusted, private: label.private };
                    let ing = self
                        .memory
                        .ingest_source(&SourceInput {
                            tenant_id: DEFAULT_TENANT_ID.into(),
                            source_id: item.clone(),
                            rev,
                            owner_id: Some(person.clone()),
                            readers,
                            label: trust,
                            kind: "lesson".into(),
                            text: body.to_string(),
                            metadata: json!({ "op": op }),
                            occurred_at: e.at,
                        })
                        .await?;
                    if !ing.deduplicated {
                        let payload = EnrichmentPayload {
                            source_embedding: None,
                            model: None,
                            assertions: vec![AssertionInput {
                                assertion_id: item.clone(),
                                kind: "lesson".into(),
                                statement: body.to_string(),
                                valid_from: Some(e.at),
                                valid_to: None,
                                confidence: 1.0,
                                embedding: None,
                                model: None,
                            }],
                            entities: Vec::new(),
                            edges: Vec::new(),
                        };
                        self.memory.commit_enrichment(DEFAULT_TENANT_ID, item, rev, None, &payload).await?;
                    }
                }
            }
            Ev::KnowledgeAdded { id, title, text, readers, integrity, author, .. } => {
                let rev = e.seq.max(1);
                let full_text = format!("{title}: {text}");
                let owner_id = author.strip_prefix("person:").map(String::from);
                let trust = TrustLabel { untrusted: integrity != "company", private: !readers.is_empty() };
                let ing = self
                    .memory
                    .ingest_source(&SourceInput {
                        tenant_id: DEFAULT_TENANT_ID.into(),
                        source_id: id.clone(),
                        rev,
                        owner_id,
                        readers: readers.clone(),
                        label: trust,
                        kind: "knowledge".into(),
                        text: full_text.clone(),
                        metadata: json!({ "title": title, "integrity": integrity }),
                        occurred_at: e.at,
                    })
                    .await?;
                if !ing.deduplicated {
                    let payload = EnrichmentPayload {
                        source_embedding: None,
                        model: None,
                        assertions: vec![AssertionInput {
                            assertion_id: id.clone(),
                            kind: "knowledge".into(),
                            statement: full_text,
                            valid_from: Some(e.at),
                            valid_to: None,
                            confidence: 1.0,
                            embedding: None,
                            model: None,
                        }],
                        entities: Vec::new(),
                        edges: Vec::new(),
                    };
                    self.memory.commit_enrichment(DEFAULT_TENANT_ID, id, rev, None, &payload).await?;
                }
            }
            Ev::Trashed { item, .. } | Ev::Removed { item, .. } => {
                if !item.starts_with("person:") {
                    self.memory.revoke_source(DEFAULT_TENANT_ID, item, None).await?;
                }
            }
            Ev::PersonErased { person } => {
                self.memory.erase_person(DEFAULT_TENANT_ID, person).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Process up to `ENRICH_BATCH_LIMIT` pending enrichment jobs, computing
    /// embeddings via `Kernel::embed` when an embedding server is configured.
    pub(crate) async fn drain_memory_enrichment(&self) -> usize {
        let mut processed = 0;
        for _ in 0..ENRICH_BATCH_LIMIT {
            let Ok(Some(job)) = self.memory.claim_next_job().await else {
                break;
            };
            processed += 1;
            let owner = match &job.owner_id {
                Some(p) => Owner::Person(p.clone()),
                None => Owner::Server,
            };
            let embed_res = self.embed(owner, &job.text).await;
            let (model, vec) = match embed_res {
                Ok(Some((m, lit))) => (Some(m), parse_vector_literal(&lit)),
                Ok(None) => (None, None),
                Err(e) => {
                    let _ = self.memory.fail_job(job.job_id, &e.to_string()).await;
                    continue;
                }
            };
            let payload = EnrichmentPayload {
                source_embedding: vec.clone(),
                model: model.clone(),
                assertions: vec![AssertionInput {
                    assertion_id: job.source_id.clone(),
                    kind: "fact".into(),
                    statement: job.text.clone(),
                    valid_from: None,
                    valid_to: None,
                    confidence: 1.0,
                    embedding: vec,
                    model,
                }],
                entities: Vec::new(),
                edges: Vec::new(),
            };
            if let Err(e) = self.memory.commit_enrichment(&job.tenant_id, &job.source_id, job.source_rev, Some(job.job_id), &payload).await {
                let _ = self.memory.fail_job(job.job_id, &e.to_string()).await;
            }
        }
        processed
    }
}

/// Spawn the background memory enrichment & watermark-gated curator loop.
pub fn schedule(k: &Arc<Kernel>) {
    let weak = Arc::downgrade(k);
    tokio::spawn(async move {
        let mut enrich_tick = tokio::time::interval(ENRICH_POLL_INTERVAL);
        let mut curator_tick = tokio::time::interval(CURATOR_POLL_INTERVAL);
        enrich_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        curator_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = enrich_tick.tick() => {
                    let Some(k) = weak.upgrade() else { break };
                    let _ = k.drain_memory_enrichment().await;
                }
                _ = curator_tick.tick() => {
                    let Some(k) = weak.upgrade() else { break };
                    if let Err(e) = k.memory.run_maintenance_cycle(DEFAULT_TENANT_ID).await {
                        tracing::debug!("memory curator cycle skipped: {e:#}");
                    }
                }
            }
        }
    });
}
