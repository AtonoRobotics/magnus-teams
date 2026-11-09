# #289: backup schedule waits from configuration

The schedule's two hardcoded waits move to `[backup]`: `fallback_wait_secs`
(default 60) for the wait used when the computed wait cannot be converted to
a sleep, and `retry_wait_secs` (default 3600) for the wait after a failed
round. Defaults keep the old behaviour byte for byte.

## Why pure wait functions

The loop itself cannot be e2e-tested: its sleeps run at hour scale, and the
first wait is at least a minute even with a stale backup on disk. So the
decisions moved into three pure functions the loop calls — `next_wait`,
`retry_wait`, `to_std_or` — and the tests pin those plus the toml keys that
feed them. A configured value is shown honoured at both ends: toml lands in
`Backup`, and the functions return exactly what they are given.

## What stays hardcoded

The `max(seconds(60))` clamp and the `minutes(10)` first-start wait are
schedule policy, not the waits the issue names; they stay as they were.
