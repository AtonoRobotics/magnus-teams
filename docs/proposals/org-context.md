# Org context

Status: plan, to build. Source: the owner's "Magnus org context build spec"
(2026-11-02, https://claude.ai/artifact/RZdR3LW355wsweaszctQ99), which
replaces the memory spec. This plan maps it onto what Magnus has and orders
the rest into steps, each its own pull request with the spec's tests for
what it builds.

## The owner's rulings (2026-11-02)

- **Erasure stays the content store.** Content already lives outside the
  log, in a store whose rows are deleted outright (deletion step 1); the log
  holds no person's words to encrypt. The spec's per-user keys for event-log
  payloads are not built: removing a person deletes their content rows,
  their user-scope documents and versions, and their permission rows, and
  relabels org documents they wrote `human:removed`.
- **Relationship memory is in context by code.** The behavior spec is the
  later revision and wins: how a person wants Magnus to work with them
  ("fewer jokes", what they delegated, standing decisions) enters every run
  of theirs by code, the one exception to "three things enter a run by
  code". It is kept in their user memory under `/users/<user>/`.
- **Org context comes before the rest of the behavior spec.** The behavior
  steps that keep notes (relationship memory, the suggestion ledger, the
  commitment ledger) sit on the bundle and user memory, not built twice.

## What exists, and what it becomes

| Today | Becomes |
| --- | --- |
| Company knowledge base (`knowledge.rs`: entries with read rights from their sources, trust levels, endorsement, pgvector or full-text search) | The bundle's conventions and entities; its read-right inheritance becomes K2 visibility over `bundle_doc_source`. "No separate knowledge base." |
| Per-person memory entries (`memory_write`, `memory_search`, the memory pass) | Per-user memory documents under `/users/<user>/` with `memory.list/read/search/write/invalidate/forget` (K4); the memory pass writes there |
| The playbook (lessons per person, versioned) | User memory: per user and per task only |
| Ontology types in the console (not built yet) | Type documents; the Cedar entity schema is generated from them |
| Console Context sections (not built yet) | Removed: the bundle replaces them |
| Connectors (each person's own sign-in; catalog manifests) | The grants K1 runs under; their search becomes `org.search`, their reads `org.fetch` |
| Recall only (no one's data held in memory; content recalled through the per-person wall) | Unchanged, and the rule for source items too: nothing mirrored, only the gated per-source content index |

## Steps

1. **The bundle.** Documents as events (`BundleWritten`, with path,
   frontmatter, body, version, actor), projected into Postgres tables that
   point at the event that made each row: documents, versions, document
   sources, read permissions, run reads. The seven kinds and their required
   fields, validated on every write (K8), `/index.md` at most 16KB (K9),
   versions with history and rollback. Tools `bundle.read`, `bundle.write`,
   `bundle.search` (keyword first), with K2 visibility over the documents'
   sources. Tests T9, T10, and T2 for bundle reads.
2. **Context assembly.** The system prompt describes the environment (tools,
   the bundle, labels) and nothing about the org. The top index is rendered
   for the requester (lines whose links are all invisible dropped, invisible
   links removed), the person's memory index appended, cached per user and
   invalidated on writes and grant changes; relationship memory by code (the
   owner's ruling). Bundle reads carry the corrections-after-read label.
   The knowledge base and playbook leave the context.
3. **Per-user memory.** `/users/<user>/` documents and index, the `memory.*`
   tools filtered by owner inside the SQL (K4, T5), "Your memory" in the app
   (view, edit, forget), existing memory entries and playbook items moved in
   once, the memory pass writing there.
4. **The org's sources.** `org.search` (each source's own search in
   parallel under the requester's grant), `org.fetch`, `entity.resolve`,
   `people.ask`; K1 grants (a scheduled run uses its owner's; survey and
   curator runs use the source's `connected_by`), K3 permission checks with
   their cache and expiry. Tests T1, T3, T4.
5. **The kernel's other rules.** K5 shared output (a Cedar action whose
   context lists the items the run read; approval when a recipient cannot
   read one), K6 refusals naming their policy, K7 untrusted triggers, K12 an
   audit event per tool call. Tests T6, T7, T8, T13.
6. **Change events and erasure.** Each connector's change mechanism or a
   poll; `source.item_changed`, `source.item_deleted`,
   `source.permissions_changed`; K10 staleness; K11 erasure cascades for a
   deleted item, a revoked grant and a removed person. Tests T11, T12.
7. **The survey and curator runs.** The survey on `source.connected` (T15);
   the curator nightly at the admin's time and on demand, with the kernel's
   brief (stale documents, corrected or rejected conventions, unread ones,
   people's edits, pending reviews).
8. **Outcomes.** Acceptance, rejection and edits recorded by the kernel;
   corrections, downstream results and rechecks by Magnus through
   `bundle.record_outcome`; cited sources in the app with "mark wrong"; owner
   review of conventions in approvals. Test T14.
9. **The knowledge base retired.** Its entries move into the bundle once; its
   code, tables and tools go.
10. **The console.** Bundle, conventions, sources (and the gated content
    index), curator, reads and overshared items, and experiments, as the
    console's Knowledge section; memory endpoints address one person's
    memory only.
11. **The experiment harness.** The task set, every routed model, the
    toggles (withhold the index, conventions, definitions; score each
    source's search), compounding across weekly snapshots, the acceptance
    audit, the metrics, shown at `/experiments`.

## Open

- The bundle follows the Open Knowledge Format v0.2 as published
  (https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md):
  index files carry no frontmatter except `okf_version: "0.2"` on the
  bundle root; `generated.at` and frontmatter `sources` (v0.2) rather than
  v0.1's `timestamp` and body citations. Where OKF says more than the spec's
  table (frontmatter forms, link syntax), step 1 follows OKF; the Magnus
  profile's required fields are the spec's.
