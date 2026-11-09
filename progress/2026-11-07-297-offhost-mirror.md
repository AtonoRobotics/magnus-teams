# #297: mirror backups off host to copy_to

## Changed

- `crates/magnus/src/config.rs`: `[backup] copy_to` (none by default)
  and `copy_timeout_secs` (300).
- `crates/magnus/src/events.rs`, `state.rs`: `BackupCopied` event and
  `last_copy` projection.
- `crates/magnus/src/backup.rs`: `back_up` mirrors the pruned directory
  via rsync when configured; the answer carries `copied`/`copy_detail`
  (null when unconfigured).
- `crates/magnus/src/api.rs`: the backups status shows `copy_to` and the
  last copy.
- Tests: `backup_copy_comes_from_config` pins the keys; the e2e mirrors
  to a local dir (new file arrives, planted stale file leaves), records
  a failed destination while the backup holds, and asserts both log rows.

## Verification

- E2E RED on pristine code (no mirror), GREEN with it; config test fails
  to compile without the new keys.
- `backup` 3/3, `magnus` lib 100/100; clippy clean on touched lines.
