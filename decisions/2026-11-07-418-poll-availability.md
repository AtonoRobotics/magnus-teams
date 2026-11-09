# Polls skip watches the person cannot use (#418)

Codex P2 on #416, confirmed: `poll_due` enumerated raw
`cfg.connectors`, so a disabled connector, a never-connected app or
a lapsed sign-in still got a paid polling run every interval — a run
whose prompt cannot carry the tool spec and whose read the boundary
would refuse. It now iterates the effective connector list (which
also picks up runtime-added and catalog connectors and their
watches) and skips people who are not connected or whose sign-in
lapsed — the same `connected`/`lapsed` predicates `app_tool_specs`
uses, so the scheduler and the prompt agree on what is usable.

Liveness (an app refusing its tools) stays a runtime failure, not a
scheduler check: probing every watch every sweep costs more than
the failed runs it would save. Rights stay with the prompt too: a
watch whose tool the person may not use still starts its run and
fails cheaply. Both are deliberate; the expensive recurring waste
was the disconnected and turned-off cases, and those start nothing.
