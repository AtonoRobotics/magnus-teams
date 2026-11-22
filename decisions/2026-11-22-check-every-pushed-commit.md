# Check every pushed commit and release the push tip

**Choice.** Each branch push runs fast CI for the tip and independently for
the earlier commits introduced by that push. Every check runs the same
`ci/fast.sh` on its own checkout. Earlier checks run independently so a
failure in one does not cancel the others or block a tip that passes.
Concurrent pushes no longer cancel each other's fast checks.

The `fast` workflow still checks the push tip. Its result gates that tip's
release on main. Earlier commits get their own check results in Actions.

**Reason.** A push can contain several commits. Previously only its tip was
checked, and a newer push could cancel the earlier push's fast check. The
owner asked for checks on every commit and releases for push tips.
