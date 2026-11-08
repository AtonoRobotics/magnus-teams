# Architecture

## 1. Shape: server + computers, clients thin

A **hosted server per workspace** is mandatory. Autonomy needs
schedules, triggers, and webhooks that fire while every laptop is
asleep. The server holds the record and runs coordination. It does
not need to hold plaintext content (see §7).

**Agent computers** execute tools and browser work. They are
persistent per workspace or per participant, sleep when idle, and are
disposable: everything that matters is on the server, so a computer
can be rebuilt without losing work (the split Grok Bot proves; Meta
Muse proves the per-user computer; the salvaged Magnus brokered kit
proves credential-free guests).

**Clients** (desktop/phone) render, approve, watch, and take over.
Heavy or sensitive computation never depends on a client being open.

**First deployment**: the workspace server runs on the **witness
host**, a new always-on server provided for this build. Phase 0–4
(build-plan) need no other hosting: watcher, record, coordination,
and the Magnus worker all live there, and agent computers run there
or are reached from there. Moving to multi-tenant cloud hosting is a
Phase 5 concern and changes no contract in this document.

## 2. The record: context built from its own events

Every interaction is an append-only **event**: message, claim, plan,
commit of work, review, decision, approval, effect. The event record
is the only source of truth.

- All state (tasks, timeline, membership, authority) is a projection
  replayed from events. Derived summaries are regenerated, never
  hand-maintained, so context cannot go stale.
- A participant's briefing on joining a task is **assembled from
  events**: goal, open claims, recent merged artifacts, standing
  decisions, each with provenance.
- Content (words, files) lives beside the log, not in it, deletable
  independently, as in Magnus. Erasure deletes content and the
  workspace encryption key; backups therefore hold only ciphertext.

Consequence: workspace context is endogenous. It is built entirely
from its own interactions and explicit promotions in. Nothing else.

## 3. Hybrid coordination

Three layers, strictly ordered:

### 3a. Deterministic kernel (trusted, minimal)

- **Claim**: first claim on a work item wins. Claims carry owner,
  scope (items/files), and an expiry.
- **Heartbeat**: renews a claim. Past expiry, anyone may take over.
  No polite deadlock, no silent collision.
- **Permissions**: who may read, write, approve, promote, per
  workspace, room, and item. Rules, not opinions.
- **Promote/merge**: an artifact becomes shared truth only via review
  plus deterministic checks. Conflicts (same item/file changed on
  two live claims) are computed, not negotiated.
- **Pause**: one flag, checked by scheduler, timers, and in-flight
  execution alike. (Magnus lesson: a gated schedule beside an ungated
  timer is not a pause.)

### 3b. Messages (the interface)

Participants talk. Coordination-relevant speech is also a kernel act:

| Message | Kernel effect |
|---|---|
| "I'll take this" | Claim with expiry |
| "Here is my plan" (≤ a few lines + files touched) | Proposal attached to the item |
| First visible work (e.g. draft PR) | Presence signal; scope becomes observable |
| "Still working" | Heartbeat |
| "Done / stopped" | Handoff: artifact + evidence + open questions + next owner |

Free chat remains free. Only these five have side effects, and the UI
shows the effect on the message ("Claimed by Codex until 14:00").

### 3c. Orchestrator (optional participant)

An agent may watch the record and propose: assignments, decomposition,
reviewer pairing, staleness. Its proposals are ordinary artifacts,
accepted or ignored by others. It has no kernel privileges and the
system must pass its coordination tests with the orchestrator absent.

### Delivery

Separate harnesses never poll. The server watches sources (repos,
inboxes, schedules) and **pushes** events into live harness sessions:
"a file in your claim was just claimed by someone else." Coordination
that arrives after the edit is archaeology.

## 4. Minimal ontology

Fixed core, about a dozen nouns: `Person, Agent, Workspace, Room,
Task, Claim, Plan, Artifact, Decision, Review, Approval, Evidence,
Message`. Verbs: `claims, proposes, revises, reviews, approves,
merges, blocks, supersedes, promotes`.

Everything else is free text/files attached to core nouns. Workspaces
may add fields; they may not redefine the core. This is the shared
language that makes vendor agents interoperable and the record
queryable. No enterprise ontology, no upfront taxonomy programme.

## 5. Work model (Git for work, not just code)

- **Commit**: finished work, signed by author, with artifact + evidence.
- **Branch**: private workspace or private draft line of work.
- **Proposal (PR-shaped)**: request to promote; plan, semantic diff,
  and checks attached.
- **Merge**: promoted after review and verification. **Revert**:
  undoing one commit without disturbing others.
- **Semantic conflict**: same fact, date, file, or decision changed in
  two live lines; shown in plain words with both sources.

Users see drafts, suggestions, and a timeline. Branch/commit language
stays under the surface.

## 6. The agent loop and its boundary

Each acting participant runs: propose → claim → execute → evidence →
review. Any state-changing or external effect passes a single
boundary, salvaged from Magnus:

1. Resolve actor, workspace, item, and claim. Fail closed if absent.
2. Record intent durably **before** the effect.
3. Check authority: workspace policy (narrowing layers only, as in
   Magnus invariants 6/8), taint (Rule of Two / audience, invariant
   4), and budget. Kernel asks (taint) accept only exact, one-use
   approvals bound to what was shown.
4. Execute with an idempotency key derived from run, step, and tool.
   After a crash, reconcile before any resend; never resend an
   unknown-outcome effect without a human or a declared-idempotent
   receiver.
5. Record the result; redact every credential in every encoding at
   this single point.

A reply with no action never ends a worker's goal. A denial returns
to the deciding model. Budgets suspend and notify; they never
silently truncate.

Completion is separate (invariant 7): criteria are written before
acting, checks re-read the world at completion time, unchecked
criteria go to a person or a *different* agent for review.

## 7. Identity, tenancy, data

- **Tenancy**: workspace is the tenant. Every event, artifact, file,
  and secret carries `workspace_id`; private content adds `person_id`.
  Enforced in the database (row-level security), not only in app code.
- **Identity**: immutable subject per person. Never a username.
  Passkeys / device keys for people. Agents act under a named owner
  and their own participant identity; audit shows both.
- **Roles**: owner, member, approver. Product roles beyond that are
  policy, not identity provider roles. Group→role mapping, where SSO
  exists, is explicit workspace configuration.
- **Encryption**: per-workspace data key, wrapped per member device.
  Server stores ciphertext for content; metadata needed for routing
  is minimised and listed. Erasure = destroy key + delete content.
- **Models**: bring-your-own-key or hosted default, chosen per
  workspace. The served model must equal the requested/pinned model
  or the run fails. Cost is priced from the served model; unknown
  price means the run does not start.

## 8. Execution environments

- **Agent computer** (trusted, visible): persistent desktop + browser,
  per workspace or participant. Takeover fences all agents on it.
- **Sandbox** (untrusted): per-run microVM, no network except an
  allow-listed proxy, no credentials. Salvaged Magnus sandbox.
- **Brokered connectors**: external apps are reached by brokered
  guests that receive a per-call token and store nothing, as in the
  Magnus brokered kit. A signed catalog manifest carries each tool's
  risk class; pins cover name, description, input **and** output
  schema, and annotations. A changed definition disables the tool.

## 9. Proactivity

Triggers: schedule, inbox, repo events, finished work, room messages.
Proactive passes run read-only by default (research, monitoring,
summarising) and end in a proposal, a draft, or silence. Speaking to
the user is itself an action through the boundary: one send path,
silence by default. This follows Dots' research/action split and Grok
Bot's single send tool, with Muse's initiative behind product §4.3.

## 10. Rules for this codebase (replaces appliance process)

Three standing rules in the new repo:

1. Claim first (kernel-enforced anyway).
2. Evidence before done (kernel-enforced anyway).
3. Time-boxed trials: any component missing its trial date is deleted,
   not extended by default.

Decisions are workspace events. There are no per-PR decision or
progress files, no session-start document stack (a generated briefing
replaces it), and no prescribed workflows in standing docs. This file
set is revised only when a principle or contract changes.
