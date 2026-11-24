# Tests run on the server's stack size

**Choice.** `ci/fast.sh` sets `RUST_MIN_STACK` to 16 MiB, the
stack the server gives its tokio workers (`cli.rs`, `WORKER_STACK_BYTES`),
so every test thread has it. The tests that drive the whole tool pipeline
or a batch of sub-agents also run their body on a 16 MiB worker
(`on_big_stacks`), so they hold without the setting too.

**Reason.** `boundary::finish_tests::a_resent_irreversible_send_is_read_back`
overflowed the test thread's default 2 MiB on the GitHub runner, though not
on Strix Beta, and the abort stopped the fast tier and the release of
0c98f35. A test is meant to fail on its own assertion, not on a stack
smaller than the one the code runs on in the server.

It is set in `ci/fast.sh`, not in `.cargo/config.toml`'s `[env]`: cargo
puts `[env]` in every crate's fingerprint, so the first try rebuilt every
crate and the fast tier ran past its ten minutes (c685a99).
