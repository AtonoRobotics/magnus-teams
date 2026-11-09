# #466 accounts to Postgres (2026-11-08)

- Rewrote `crates/magnus/src/accounts.rs` on Postgres (same behavior, async): `migrate_sql` + `import_json`, lazy client, advisory-locked switches, SQL single-use semantics, store-checked sessions.
- Wired `migrate` (tables + one-time JSON import), CLI commands on `database.url` (async), ~140 mechanical `.await`s across `who`/`owner`/`developer`/`assigned_channel` and the accounts call sites.
- Backup: table-scoped accounts dump in the doc, data-only restore into migrated tables (pre-wiped on its own connection for reruns), `check()` validation, detail string extended.
- Debugging notes: scratch e2e spawns `target/debug/magnus-appliance` from the invariants crate — `cargo build --workspace --bins` does NOT rebuild it; run `cargo build -p magnus-invariants --bins` too (a stale binary cost one red herring here). The sign-in 401 during e2e was the stale directory cache, fixed by fresh routing reads + refresh-on-issue (proven by restart/no-restart probe).
- E2E: backup 4/4 (incl. new restore-keeps-sign-in), local_people 1/1, admin_local_sign_in 1/1, admin_people 1/1, people 5/5, cold_start 5/5. Lib 144/144, clippy clean on touched code.
- Closes #466 and #469. #470 (uppercase ids) stays separate: backend-agnostic, own PR.
