# Fast runs queue instead of cancelling

**Choice.** `fast.yml` keeps its one concurrency group per branch but sets `cancel-in-progress: false`. A run in progress always finishes. A push that arrives meanwhile waits. GitHub keeps one waiting run per group, so a later push replaces the one waiting, never the one running. The waiting run is for the newest commit, which contains every commit before it. `ci/release-gate-check.sh` fails if the setting comes undone.

**Why.** With `cancel-in-progress: true`, each push cancelled the run still going. On 2026-11-27, pushes to main came about three minutes apart, and a fast run takes five to ten minutes. Every fast run on main from 14:29 to 15:59 was cancelled. Because a release waits for a passing fast run (decisions/2026-11-20-release-waits-for-fast.md), nothing reached the server for that time, and no commit got a verdict.

**Not one run per commit.** Running every commit's checks side by side, in a group per commit, would give each commit its own verdict. But runs that finish out of order would start releases out of order, and an older commit could replace a newer one on the server. One run at a time keeps releases in commit order. A commit skipped in the queue is checked as part of the next one.

**Cost.** A push can wait up to one run, about five to ten minutes, before its own run starts.

**Replaced** the same day by decisions/2026-11-27-fast-runs-fifo.md: one waiting run per group dropped the pushes in between.
