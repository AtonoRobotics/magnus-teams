# Tests have no line limit

**Choice.** The 800-line limit (`ci/ratchet.sh size`) applies to source
files only. A file in a `tests/` directory, or named `*_tests.rs` or
`*_test.rs`, is exempt. The owner chose this on 2026-11-24.

**Reason.** The limit earns its keep in the kernel: its pressure split
connectors.rs into modules and moved the work's fold into state::work,
and a module that fits in a read is one an agent reads whole. In a test
file it buys nothing: tests are read one at a time, and a split there is
chosen by line count, not by meaning. Under `tests/invariants/` it also
cost an owner round trip: r9-staging, approved, put inv4_taint.rs at 812
lines and was held back until its tests were moved to another file for
no other reason.
