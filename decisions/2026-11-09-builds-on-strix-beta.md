# Builds and test suites run on Strix Beta

The owner, 2026-11-09: "Strixalpha and strixbeta are now development and CI
hosts" and "move the builds to strix-beta".

Choice: heavy builds, the workspace test suite and acceptance runs (the
autonomy scenarios among them) run on Strix Beta through
`ci/on-strix-beta.sh NAME COMMAND...`. It copies the checkout (no `.git`,
`target/` or `node_modules/`) to `~/magnus-teams-ci/NAME/` there and runs the
command with one shared `CARGO_TARGET_DIR` and the magnus-teams test database,
its own Postgres on 127.0.0.1:54361 (never magnus-claude's).

The release job stays on magnus-server's runner, because it publishes to the
server on that machine.

Reason: every agent's worktree kept its own `target/` on magnus-server, and
the disk filled again and again, which killed a release run mid-job. Strix
Beta has 2 TB and 32 cores. The server's machine keeps only what the server
and the release need.

Strix Beta is reached at its LAN address (samuel@192.168.0.51, host key alias
strix-beta), because the tailnet name hung.
