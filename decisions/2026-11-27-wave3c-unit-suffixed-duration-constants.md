# Wave 3C: Unit-Suffixed Duration Constants

## Decision
Renamed bare-named `Duration` constants across `crates/magnus/src/` to include explicit units (`_SECS` / `_MS`):
- `HELD_FOR` → `HELD_FOR_SECS`, `ROUTINE_TICK` → `ROUTINE_TICK_SECS`, `JOB_WAIT` → `JOB_WAIT_SECS`, `ONLINE_GRACE` → `ONLINE_GRACE_SECS`, `WORLD_MAP_SYNC_INTERVAL` → `WORLD_MAP_SYNC_SECS`, `NEXT_WAIT` → `NEXT_WAIT_SECS`, `ACTIVE_POLL_WAIT` → `ACTIVE_POLL_WAIT_SECS`, `YIELD_WAIT` → `YIELD_WAIT_SECS`, `RESUME_WINDOW` → `RESUME_WINDOW_SECS`, `HELD_CALL_WAIT` → `HELD_CALL_WAIT_SECS`, `EMBED_TIMEOUT` → `EMBED_TIMEOUT_MS`, `CONNECT_WAIT` → `CONNECT_WAIT_SECS`, `CALL_WAIT` → `CALL_WAIT_SECS`, `FETCH_WAIT` → `FETCH_WAIT_SECS`.

## Reason
Per `AGENTS.md` and the Magnus Code Standard, every duration or limit constant carries its unit in its name so call sites and reviews are unambiguous at a glance.
