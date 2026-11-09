# #433 console scope (2026-11-07)

- Extended `docs/proposals/embedded-auth.md` with "Administering from the console": Users, Appliance and Context slices with live acceptance, doc-claim reconciliation, and the slot for the per-request role check and `magnus_root` re-decision. Extended #429's body with the console slices (each its own PR from master).
- Verified before scoping: 71 console routes with all five sections and tabs wired, UI complete with no stubs, the live console healthy (shell 200, unauth 401, no unit errors in 24h) — the gap is runtime usability (invite mail most concretely), not missing screens.
