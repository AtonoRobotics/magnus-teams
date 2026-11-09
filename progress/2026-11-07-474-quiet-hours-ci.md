# Quiet-hours CI fixture (2026-11-07, #474)

- #471 run 42427138935: 706/707 passed; interruption fixture started in hour23 and was processed just after quiet ended at midnight. Recorded notify_at was already past at assertion.
- Removed the artificial 23:59 working-hours gap and gave the quiet fixture headroom around every minute. Added deterministic coverage of all 1440 start minutes and delays through 59 minutes, including midnight.
- `cargo test -p magnus-scratch --test proactive -- --nocapture`: 15/15 passed, including all-minute/delayed-window coverage and actual triggered interruption. Product binaries were unchanged; no failed job was blindly rerun.
