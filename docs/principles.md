# Principles

Principles stand until the mission changes. Processes stand while useful.
Workflows stand until a variable changes. Encode only principles here.

## P1. Agency inside strict gates

Prescribe the record, never the work. Before starting: a claim. Before
done: evidence. Before shared: review. Inside those gates any path,
tool, order, or decomposition an agent chooses is legal.

Judge outcomes, not steps: finished, verified, collided, cost.

## P2. Shared record, not shared brain

A workspace shares work objects: tasks, plans, artifacts, decisions.
It does not share ambient memory or organisational context.
Nothing is team truth because an agent believes it. It is team truth
because it was promoted, reviewed, and merged, with provenance.

Private by default. Shared by explicit promotion. References travel
between workspaces, never raw memory.

## P3. Distributed planning

No central planner. Any participant, human or agent, may propose a
plan, claim part of it, criticise it, or propose a rival. Humans decide
ties. Order emerges from the record, as in Git.

An orchestrator may exist only as a participant with no special write
power. It suggests routing, decomposition, stale work, and reviewers.
If it cannot be removed without breaking the workspace, it has become
the planner and violates this principle.

## P4. Provenance on everything

Every artifact, fact, and action names its author (person or agent),
its time, and its source. `blame` must always be answerable. An
unattributed change is not a change; it is a defect.

## P5. Verified authority, verified actions

No effect without recorded intent and authority. No completion on an
agent's report alone: done is checked against the world by a different
participant or a deterministic check. This is Magnus's original
contribution and is carried over unchanged.

Standing authority is scoped (tool, audience, budget, time), expiring,
and revocable in one action (pause). Pause stops schedules, timers,
and in-flight runs.

## P6. Deterministic kernel, disposable workflows

Claims, leases, expiry, permissions, merges, and conflict outcomes are
deterministic: same inputs, same result. They are boring and testable.

Workflows are generated per task, used, and thrown away. A workflow
becomes a standing process only when a human deliberately promotes it.
Any rule that names steps instead of gates is presumed expired.

## P7. Proactivity behind a fence

Proactive discovery is read-only and proposes. Action is a separate
permission. Initiative must never silently become effect. The level
per goal is user-controlled: off, suggest, draft, act-standing.

## P8. Simple enough for non-technical people

One app, rooms, people, approvals in plain words. Internal vocabulary
(policy, taint, boundary, run, lease) never appears in the product.
Defaults propose the plan, the people, and the next step. The user agrees.

## Red lines (fail the build)

- An agent grants itself authority or widens another's.
- A credential reaches a model, a transcript shown to other
  participants, or a log without redaction.
- The same external effect repeats after a crash or retry.
- An action executes before its intent is durably recorded.
- Private workspace content appears in a team workspace without an
  explicit promote by its owner.
