# Reviews surface tasks as observations, not proposals (#382)

Codex P2 on #370: review.md told reviews to propose tasks via
`propose_task`, which is chat-only (offered for chat turns, denied in
tasks, and its card lives in the thread). Exposing it would need a new
background proposal card path (inbox kind, accept flow, rendering):
filed as #393. The instruction now says what works: automations via
`propose_schedule` (a run record, available everywhere), and tasks to
start via `observe`, which the person sees and starts themselves.
