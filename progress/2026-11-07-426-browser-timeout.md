# Dead addresses: proxy timeout plus a deterministic DNS test (#426)

`browser/destinations.js`: `connect()` tries each checked address with
a 10s cap, destroys the timed-out socket (swallowing its late error so
the sidecar cannot crash), moves on, and errors when all are dead; it
is exported for tests with an `opts` ({ dial, ms }) seam. New
`browser/destinations.test.js` (`node:test`, no deps): dead-first falls
through to the live address, all-dead errors, both asserting they
settle in under 5s. `crates/magnus/tests/tools.rs`: the private-name
test navigates to `localhost` instead of the machine's name. The
release-checks Browser sidecar step now runs `node --test
browser/destinations.test.js`, naming the file: a bare directory
argument runs the package's main, and the runner's older node does
not expand `**` patterns.
The 90s navigation-budget bump is reverted: `sidecar.js` and
`browser.rs` are master-identical again.

Verification: the new node test fails pre-fix (both cases time out at
the 20s harness budget — the never-settles bug) and passes post-fix
(2/2 in 454ms). `node --check` passes on both JS files. The Rust DNS
test, reproduced hanging locally on the base (90.35s, the CI flake),
passes in 1.01s with the fix. Full `tools.rs`: 13 pass;
`sandbox_desktop_takes_screenshots_and_types` fails identically on the
untouched base (no sandbox guest in this environment — CI builds it).
