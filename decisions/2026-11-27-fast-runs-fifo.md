# Fast runs are a first-in, first-out queue

Replaces decisions/2026-11-27-fast-runs-queue.md, made earlier the same day.

**Choice.** Every push gets its own fast run, and runs complete in the order they were pushed.

- `fast.yml`'s concurrency group is one per commit (`fast-<ref>-<sha>`, `cancel-in-progress: false`). No push cancels or replaces another's run, so each push gets its own verdict.
- The checks of all waiting pushes run in parallel on hosted runners. A last job, `in-order`, runs after the checks whatever their verdict, and waits (`ci/fast-wait-previous.sh`) until the fast run for the commit the branch pointed to before the push (`github.event.before`) has completed. It polls every 20 s and gives up after 30 minutes. When that run does not exist (a new branch, a manual run, a force push), it does not wait.
- The run's verdict is the checks' verdict. Since each run completes only after the one before it, `release.yml`, which starts on a fast run's completion, sees them in push order. Its single release group keeps the newest waiting release, which is then also the newest commit, so an older commit never replaces a newer one on the server.

`ci/release-gate-check.sh` fails if any of this comes undone.

**Why.** The first fix, a single queue per branch, stopped a push from cancelling the run in progress. But GitHub keeps only one waiting run per concurrency group: when five pushes landed during one run, four were dropped and only the newest was checked. A failure could not be traced to its push, and those four pushes had no verdict. A group per commit drops nothing. Running every push in parallel without ordering would let runs finish out of order, and a release started by an older commit's run could replace a newer commit on the server. Waiting for the previous run keeps the order without delaying the checks themselves.

**Limits.** GitHub starts one run per push, not per commit, so several commits sent in one `git push` share one verdict. If a run is stuck past the 30-minute limit, the run after it fails its `in-order` job and is not released; the push after that waits on a completed run and goes on.

**Cost.** One extra small hosted job per push, which waits only while the previous run is still going. The repository is public, so hosted runner minutes are free.
