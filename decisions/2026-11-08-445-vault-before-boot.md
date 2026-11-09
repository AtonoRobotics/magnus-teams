# Harness seeds the vault before boot (2026-11-08, #445)

Outside working hours the overnight review starts in the first sweep
and could reach the model before the harness's `put_secret` landed:
the adapter answers `AuthExpired`, the review suspends, and a
suspended review holds its per-person review slot forever (it releases
only on terminal events) — so the proactive tests waited 60s for a
running review that never came, depending on the time of day.

- `Appliance::start` writes the provider key, connector secrets and
  extra secrets to the vault file (with Magnus's own `Vault`) before
  spawning. Reason: the race is boot-vs-key, so the key must win by
  construction, for every suite, not by timing. The API puts still
  run, storing the same values and recording them in the log, so
  nothing else observes a difference; deliberately-missing secrets
  stay missing.
- No product change: a review with no credential still suspends with
  its notice, and resume stays manual. Reason: that is the shipped
  contract ("resume it when this is fixed"); auto-resume on key
  arrival is a separate product decision, not a flaky-test fix.
