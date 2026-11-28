# Every commit is released, in commit order

**Choice.** `release.yml` has no concurrency group. Its first job, `queue` (`ci/release-queue.sh`), on a hosted runner, decides whether the run builds and publishes:

1. A commit whose fast run did not pass is not released. This decision is immediate.
2. Every other commit is released, in commit order. The run waits until every release run created before it has completed, then builds and publishes. It polls every 30 s and gives up after 5.5 hours, under the hosted runner's six-hour job limit. No run cancels, replaces or skips another, and two never build or publish at once. Fast runs complete in push order (decisions/2026-11-27-fast-runs-fifo.md), and each one's completion creates its release run, so release runs are created, wait and release in push order.

`build` needs `queue` and runs only on its word, and `publish` follows `build` as before. `ci/release-queue-check.sh`, run by `ci/fast.sh`, checks the script's decisions against a fake `gh`, including that a commit is released even when later commits have passed, and checks the workflow's wiring.

**Why.** The owner, 2026-11-28: "It is suppose to be every release in order of commit." The release's single concurrency group (decisions/2026-11-20-release-waits-for-fast.md) held one waiting run, and each newer passing commit cancelled the release still waiting. The release runs for 7bf77fa and b7cdb25 on 2026-11-27 ended "cancelled". A first version of this queue, the same day, released only the newest passed commit and marked older ones "superseded". The owner rejected that: each commit gets its own release.

**Cost.** A release takes 8 to 15 minutes. When commits pass faster than that, the queue grows, and the server runs each commit in turn, behind main by the length of the queue. A run that waits over 5.5 hours fails its queue job, and that commit is not released. The runs behind it go on. Every release run ends with one of three answers: released, not released because its fast run failed, or not released because it waited past the limit.
