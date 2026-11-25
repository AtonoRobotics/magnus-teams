# Deep tests run on the server's stack size

**Choice.** The tests that drive the whole tool pipeline or a batch of
sub-agents (`boundary::finish_tests`, `sub_agents::batch_tests`) run their
body on a tokio worker with the 16 MiB stack the server gives its workers
(`cli.rs`, `WORKER_STACK_BYTES`), through `on_big_stacks`. They pass with the
test thread's own stack cut to 1 MiB. No global stack floor is set.

**Reason.** `boundary::finish_tests::a_resent_irreversible_send_is_read_back`
overflowed the test thread's default 2 MiB on the GitHub runner, though not
on Strix Beta, and the abort stopped the fast tier and the release of
0c98f35. A global floor was tried and dropped: in `.cargo/config.toml`'s
`[env]` it is in every crate's fingerprint and rebuilt them all (c685a99);
exported by `ci/fast.sh` (766dfae) it doubled the lib tests on the runner,
157 s to 300 s, though not on Strix Beta, and both runs passed the fast
tier's ten minutes.
