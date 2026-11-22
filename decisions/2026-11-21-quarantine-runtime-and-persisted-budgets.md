# CaMeL opaque-handle quarantine runtime and persisted security budgets (W1.1, W1.4)

**Choice.**
1. Wire `crates/magnus/src/quarantine_runtime.rs` into `schedule_sweep.rs::fire_schedule` and `boundary.rs::record_from` for unattended schedules whose routine-scoped **writable** envelope reaches someone other than the owner (excluding read-only `Host` URLs). Default extraction allows only `boolean`, `number`/`integer`, and `enum`; free text is stored behind kernel opaque handles (`$q1`, `$q2`, ...) that are rejected in destination fields and pass through `outgoing_finding` + action review upon body expansion. Descendant runs (`requester_run` lineage) inherit quarantine.
2. Add top-level `Ev::QueryGateRecorded` and `Ev::SafetyBlocked` (in `crates/magnus/src/events/security.rs`, never nested in existing enums so older binaries skip them cleanly on rollback) folded into `State.security` (`SecurityLedger`) with person-erasure support (`Forget`). Key `query_gate` budgets by the causing user turn or schedule firing (`requester.task`).

**Reason.** Closes the unwired-quarantine gap with true CaMeL handle isolation and prevents server restarts from resetting tainted query budgets or unblocking previously denied outbound sends.
