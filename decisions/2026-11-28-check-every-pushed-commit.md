# Every commit of a push is checked, the tip alone gates the release

Amends decisions/2026-11-27-fast-runs-fifo.md, whose "Limits" said several
commits sent in one `git push` share one verdict. Builds on the unmerged
codex/check-every-magnus-commit (2026-11-22), rebased onto the FIFO queue.

**Choice.** `fast-earlier.yml` runs on every push. A planning job lists the
commits the push brought in besides its tip (`ci/push-earlier-commits.py`:
`before..after`, or the payload's commits for a new branch). A matrix job
then runs all of `ci/fast.sh` on each of them, in parallel, and puts a
`fast-earlier` commit status on the commit (`ci/commit-status.sh`): pending
while it waits, then success or failure. A push of one commit has nothing to
list and the matrix is skipped.

**Not a gate.** The workflow is named `fast-earlier`, not `fast`, so
`release.yml` does not wait for it. The tip's release waits for the tip's
`fast` run alone, as before, and the FIFO order and the `in-order` job are
unchanged. A failing earlier commit does not hold up a push, and one commit's
failure does not cancel the others' checks (`fail-fast: false`).
`ci/release-gate-check.sh` fails if the workflow is renamed, loses any of
this, or `release.yml` starts waiting for it.

**Why.** On 2026-11-27, `f9b98a2`, `3654425` and `cbe8ff8` were pushed with
`85dc2aa` and never ran. The test that `85dc2aa` broke
(`membership_tests::removing_a_member_ends_their_running_tasks_and_leaves_team_files`)
could not be traced to a commit of its own.

**Limits.**
- A push of more than 256 earlier commits fails the planning job, loudly,
  rather than leaving some unchecked.
- For a new branch GitHub's payload lists at most 20 commits, so only those
  are checked there.
- The status is a verdict on the commit, not a block: nothing requires it
  before a merge.

**Cost.** One full `ci/fast.sh` (about ten minutes of hosted runner) per
earlier commit. The repository is public, so hosted runner minutes are free.
