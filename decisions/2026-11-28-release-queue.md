# Releases are a queue that cancels nothing

**Choice.** `release.yml` has no concurrency group. Its first job, `queue` (`ci/release-queue.sh`), on a hosted runner, decides whether the run builds and publishes:

1. A commit whose fast run did not pass is not released. This decision is immediate.
2. Otherwise the run waits until every release run created before it has completed: it polls every 30 s and gives up after 4 hours. No run cancels or replaces another, and two never build or publish at once. Fast runs complete in push order (decisions/2026-11-27-fast-runs-fifo.md), and each one's completion creates its release run, so release runs are created and served in push order.
3. When its turn comes, a fast run's release is superseded if a later push to the same branch has already passed its fast run. The run then completes successfully, without building, and its summary names that later commit, whose release comes next and contains this one. A manual run is never superseded.

`build` needs `queue` and runs only on its word, and `publish` follows `build` as before. `ci/release-queue-check.sh`, run by `ci/fast.sh`, checks the script's decisions against a fake `gh` and checks the workflow's wiring.

**Why.** The release's single concurrency group (decisions/2026-11-20-release-waits-for-fast.md) held one waiting run. Each newer passing commit cancelled the release still waiting. The release runs for 7bf77fa and b7cdb25 on 2026-11-27 ended "cancelled". A release takes 8 to 15 minutes, so on a busy day most commits' releases showed as cancelled, and whether a commit had reached the server could only be guessed. Now every release run ends with one of three plain answers: released, not released because its checks failed, or included in the named later commit's release.

**Why supersede, not release every commit.** A release builds on Strix Beta and installs on the server in 8 to 15 minutes. Pushes have come every 3 minutes. Releasing each commit in turn would put the server further behind with every push, and each install would replace the one before it within minutes. Superseding keeps the server on the newest passed commit and leaves the queue short. No commit's code is skipped: the later release holds it.

**Limits.** If the later commit's release fails in its build or publish, the superseded commit has not reached the server either. The next push's release carries both. A release run stuck in GitHub's queue for 4 hours fails the queue job of the run behind it, and the run after that goes on.
