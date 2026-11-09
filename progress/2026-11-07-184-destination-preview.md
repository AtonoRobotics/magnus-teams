# Authorize destination-preview reads (#184)

`destinations()` authorized nothing before reading: a send could pull a
draft through `get_draft` even when a task layer forbade that read, with
no pin check and no audit. The preview read now resolves the read tool's
facts (unknown/unclassified denies), checks connector access and the
tool pin (changed/disabled denies), and takes its own policy decision at
the task layer with the run's taint; Deny and Ask both deny the send with
the read and rule named. Every preview emits a `destinations`-stage
Decision, so the run's log shows what was read and why.

Tests: `destination_preview.rs` (task forbid denies with no provider
call; allow leaves its Decision; changed read definition denies) 3/3,
each red without the fix. No regressions: lib 111, sent/directory
provenance, draft_lock, scoped_drafts, cancel_report all green.
