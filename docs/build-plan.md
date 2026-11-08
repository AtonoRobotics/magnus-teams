# Build Plan

Assumption: agent-speed build (this repo's Magnus was built in about a
day). Estimates are elapsed time for one coordinating person plus
agents, excluding third-party waits (OAuth verification, app signing,
store review), which are noted where they gate.

## Phase 0 — New repo, smallest kernel (day 1)

New repo, three standing rules (architecture §10). Everything in
Phases 0–4 runs on the **witness host**; no cloud account, no
appliance image. Build only:

- A watcher service, deployed on the witness host, for one GitHub
  repo: issues, branches, PRs, comments, labels.
- The deterministic kernel in miniature: claim, heartbeat, expiry,
  conflict detection on files touched. GitHub comments/labels and a
  draft PR are the message transport (architecture §3).
- Two harnesses: Muse and Codex. Each can send the five
  messages and receives pushed events mid-session.
- A read-only status page: per issue — owner, branch, files, last
  heartbeat, state.

Deliverable: coordination over the Magnus repo's existing `queue`,
under its existing rules for any Magnus change itself. The new repo's
code is untouched by Magnus rules.

## Phase 1 — Live trial on Magnus (days 2–3)

- Pick two independent `queue` issues. Run Muse and Codex
  simultaneously, each under its harness. The owner supervises only.
- Record, without exception: collisions, polite quits, manual
  copy-paste events, time-to-first-visible-work.
- Fix only kernel defects the trial exposes. No features.

**Pass**: zero collisions/quits; zero pastes; status page alone
answers who/what/checked. **Fail**: the failure list becomes the next
issues; Phase 2 does not start.

## Phase 2 — Own record and room (week 2)

Only when Phase 1 passes and GitHub-as-record is visibly the bottleneck:

- Event record + projections (architecture §2), minimal ontology (§4),
  rooms, timeline, promote. GitHub becomes an integration, not the store.
- Desktop app v1: rooms, work, approvals, private space (§4 of
  product). No agent computer yet (watch = status page + draft links).
- Identity: immutable subject, passkey sign-in. Tenancy by workspace.

## Phase 3 — Trust port from Magnus (weeks 2–4)

Strangler order. Each item ports its invariant tests first, then its
code, then runs on live workspace work before the next item moves:

1. Completion/verification (run/verify) — done means checked.
2. Boundary (intent→policy/taint→idempotent effect) for workspace
   actions and connectors.
3. Vault + single-point redaction (all encodings, all credentials).
4. Sandbox + brokered kit + signed catalog with full pinning.
5. Policy narrowing layers, hidden behind templates.

## Phase 4 — Magnus worker + agent computer (weeks 4–6)

- One persistent agent computer per workspace: watch, take over
  (fence), sleep/wake.
- Magnus worker with one real job on the Magnus repo (e.g. red-build
  watcher): role, standing authority, proactive read-only passes,
  proposals, evidence reports.
- Harness for a third agent (Grok or Claude) to prove neutrality.

## Phase 5 — Team product (after Phase 4)

Invites, multiple workspaces, sharing by promote across workspaces,
hosted model option, connector breadth. Third-party gates dominate
elapsed time here (OAuth verification, signing): allow 1–3 weeks of
waiting, overlapped with Phase 4.

## Salvage map (from the 2026-11-08 Magnus review)

| Magnus source | Verdict |
|---|---|
| `tests/invariants/` (8 files, 52 tests) | Keep as contract. Port before code, Phase 3. Do not edit without owner approval, as today. |
| `crates/magnus/src/boundary.rs` | Keep code, adapted to kernel (§6). |
| `crates/magnus/src/run/` (loop, verify, reviewer) | Keep code. Fix: reviewer must re-read world; verifier must read before verdict. |
| `crates/magnus/src/vault.rs` | Keep code. Fix: padded URL-safe, percent, JSON-escaped redaction; all secrets, not one. |
| `crates/magnus/src/policy.rs` + grants | Keep design. Hide behind templates. Fix grant auto-cover inconsistency. |
| `crates/magnus/src/log.rs`, `events.rs` | Keep design (append-only + content beside log). Rebuild multi-tenant with RLS and per-workspace keys. |
| `crates/magnus/src/sandbox.rs`, `sandbox/guest` | Keep code as the untrusted runner. |
| `brokered/kit`, `brokered/*` | Keep pattern and kit. Pin output schema + annotations. |
| `crates/adapter-proto`, adapters | Keep protocol. Add served==pinned enforcement and served-model pricing. |
| `crates/magnus/src/backup.rs`, `update.rs`, appliance packaging | Leave. Replaced by hosted deploy + per-workspace encryption erasure. |
| Admin console (separate process) | Leave. Settings merge into the one app. |
| Keycloak/OIDC username identity | Leave. Immutable subject + passkeys. |
| `docs/`, `decisions/`, `progress/` process | Leave as archive. New repo: architecture §10. |
| Tests in `tests/scratch/` (tightened invariants) | Promote into the ported contract in Phase 3, or delete. No permanent scratch tier. |

## Risks

- **Dogfooding bias**: the trial team is technical. Simplicity
  (product §4.4) needs a separate non-technical trial before Phase 5.
- **Two repos drifting**: Magnus keeps shipping during Phases 1–3.
  Rule: Magnus fixes continue under Magnus rules; nothing in the new
  repo blocks them; salvage ports take Magnus `master` at port time.
- **Model/agent bias to workflows**: every design review asks "does
  this name steps or gates?" Steps are sent back (principles P6).
- **Old Magnus defects carried over**: the review's highs (timer
  pause bypass, backup erasure, unpinned served model) must be closed
  by the ported tests in Phase 3, not re-implemented.

## Expansion rule

A phase starts only when the previous phase's pain is observed on
live work, not anticipated. Anything that misses its phase window is
deleted and may be re-proposed from evidence.
