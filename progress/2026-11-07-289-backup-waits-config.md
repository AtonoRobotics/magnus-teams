# #289: backup schedule waits configurable

## Changed

- `crates/magnus/src/config.rs`: `[backup]` gains `fallback_wait_secs`
  (default 60) and `retry_wait_secs` (default 3600), with the same
  serde-default shape as `every_hours`/`keep_days`.
- `crates/magnus/src/backup.rs`: `schedule()` reads both from the config;
  the wait computation moved into pure `next_wait`/`retry_wait`/`to_std_or`
  so it is testable without hour-long sleeps.
- Tests: `to_std_or` honours a configured fallback (7, not 60);
  `retry_wait` returns a configured 120s on stale or missing backups and
  nothing on a fresh one; `next_wait` counts from the last backup;
  `backup_waits_come_from_config` pins the toml keys and the defaults.

## Verification

- `cargo test -p magnus --lib backup`: 4/4 green.
- `cargo test -p magnus --lib`: 97 passed, 0 failed.
- Clippy on the touched lines: clean (the `adapters.rs` never-loop error
  and the `list(&PathBuf)` warning are pre-existing on master).
