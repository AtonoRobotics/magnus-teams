# #315: a throttled registration is retried, not failed

## What changed

- `crates/magnus/src/connectors.rs`: `register_client` tries up to three
  times on 429 or 5xx, waiting out `Retry-After` (seconds, capped at 10 s)
  else 1 s, 2 s. Other refusals still fail at once. New unit-pinned
  `registration_backoff` helper.
- `tests/scratch/src/oauth_app.rs`: new `throttle_registrations` flag —
  the fake app's first N registrations answer 429.
- `tests/scratch/tests/connector_lifecycle.rs`: a sign-in past one 429
  succeeds and registers exactly once.

## Verification

- New test failed before the fix with the production message (`refused to
  register Magnus: HTTP 429 Too Many Requests`) and passes after.
- `connector_lifecycle` 5/5, `magnus` lib 90/90 locally; no new clippy
  lints on the changed lines.
