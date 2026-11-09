# #445 vault seeded before boot (2026-11-08)

Reproduced first, at 17:57 UTC: `a_task_ending_during_an_active_review…`
timed out after 60s waiting for bob's boot review, exactly as filed.

`Appliance::start` now seeds the vault (provider key, connector and
extra secrets, minus deliberately-missing ones) before spawning; the
API puts still store the same values and emit their events. No new
test: the existing proactive tests are the failing tests (observed
red before, green after).

Verified: `proactive` 15/15 out-of-hours, plus `kill_switch`,
`models`, `model_retries`, `aux_budget`, `directory_provenance` and
`cold_start_proposals` green. CI runs the invariants and scratch
packages for this path.
