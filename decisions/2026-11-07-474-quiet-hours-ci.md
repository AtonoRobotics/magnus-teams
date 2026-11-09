# Bound time-sensitive proactive fixtures (2026-11-07, #474)

- All-day test working hours use equal endpoints, which the existing production window predicate defines as a full 24 hours. The former 00:00–23:59 fixture excluded its last minute.
- The quiet interruption fixture uses a twelve-hour window centered on the current minute. A bounded review may cross an hour boundary or midnight without leaving it.
- Product hour interpretation, notifications and agent behavior are unchanged. This fixes the concrete #471 CI failure instead of rerunning a time-dependent test unchanged.
