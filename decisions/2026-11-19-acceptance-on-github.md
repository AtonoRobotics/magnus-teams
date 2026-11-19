# Acceptance can run on GitHub, when asked

The owner asked for the heavy suites to run on GitHub (2026-11-19).
`.github/workflows/acceptance.yml` runs the suites of `ci/acceptance.sh`
that need no KVM (invariants, autonomy, injection, update) and the scratch
tests of asks, the send reviewer and a definition changed under its pin, one
job each on a hosted runner with the fast tier's Postgres.

It runs only when asked: by hand (workflow_dispatch, with the suites to run),
or on a pull request labelled `acceptance` (and on that pull request's later
pushes while the label stays). It is never a required check, so heavy
suites still never gate a push. The suites that need KVM and guest images
(the computer's kill switch, the desktop look, the eighth autonomy scenario)
stay on Strix Beta.

Owner patches waiting in `docs/proposals/*-invariants.patch` are applied to
the run's checkout first, unless the run is told not to: a change and the
invariant changes it needs are tested together before the owner applies
them. Nothing is committed; `tests/invariants/` in the repository changes
only by the owner's `git apply`.

The invariants and autonomy suites also run on the pull request's base
(`invariants-base`, `autonomy-base`), without the patches, so a failure
the change did not cause shows in both and is not mistaken for its own.
Every invariant file runs (`--no-fail-fast`), so the ones that need a
computer (no `magnus` group or firecracker on a hosted runner) hide none of
the others.
