# The fast tier's timeout is fifteen minutes

**Choice.** `.github/workflows/fast.yml` cancels a run after 15 minutes,
not 10.

**Reason.** Hosted runners differ in speed. The magnus lib tests at
6eea141 took 156 s on one runner and 285 s on another the same evening,
with the same per-test times otherwise (median ratio 0.96 against the
merged code at 07fda4f, timed per test on both). Three runs of 07fda4f and
its parents passed every test and were cancelled at ten minutes, which held
the release. Fifteen minutes still fails a hung test long before GitHub's
own limit. The target stays five minutes on a warm cache; bringing the
lib tests back toward it is separate work.
