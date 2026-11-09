# Poll marks retry a hiccuping event store (#438)

`crates/magnus/src/proactive.rs`: new `emit_poll_mark` helper
(three attempts, 100/200ms backoff) used by the start mark in
start_poll and the answer mark in note_watched, each warning
loudly on final failure. Unit test
`poll_mark_emit_retries_transient_failures` failed before the
retry (single attempt) and passes after: fail-fail-ok takes three
attempts, always-fail errors after three, first-try success takes
one.

Verification: lib suite 123 pass, warning-clean on the new code
(the Ended private-in-public and controls/policy_admin warnings
are pre-existing); watch_poll file green end to end.
