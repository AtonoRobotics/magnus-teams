# Every blocked send stays blocked, not only the latest

**Choice.** For each run and tool, the server keeps every blocked message's
hash with its reason (`security_ledger::Blocked`, up to
`MAX_BLOCKED_SENDS_PER_TOOL`, oldest dropped first), in memory and folded
from `SafetyBlocked` events. A send whose hash was blocked before is
`Unchanged` and stays blocked; any other is `Revised` and reviewed with the
latest reason. The event kind is unchanged.

**Reason.** Only the last blocked hash was kept, so block A, block B, then
send A again counted A as a revision: the safety model was asked again and
could answer differently. Third review, 2026-11-23, smaller fix.
