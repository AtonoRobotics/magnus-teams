# Wake backstop (#386)

The sweep picks up ended tasks whose fast wake was dropped: the newest
unmarked task per person, ended over thirty seconds ago, gets an event
review naming it. The event computation now carries `(reason, ended)`
pairs so the backstop's `Ended` reaches `start_review`.

- `crates/magnus/src/proactive.rs`: backstop source in the sweep's
  event computation; tuple plumbing for the `ended` parameter.
- `tests/scratch/tests/proactive.rs`: the backstop test (fails
  without: 90s timeout, no review names the task); the #383 test's
  comment updated (wakes are dropped for the backstop, not lost).

Verified: the new test fails before (timeout) and passes after
(~31s, backstop fires at the grace); full `proactive` suite 7/7.

Note for #392 (one-review-381, in flight): it restructures this same
block event-first. Whichever merges second reintegrates — the
backstop source moves into the new event computation unchanged.
