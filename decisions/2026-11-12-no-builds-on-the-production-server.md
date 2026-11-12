# No builds or tests on the production server

**Choice.** `magnus-server` is the production server, and nothing is built or tested on it. All builds and test suites run on Strix Beta through `ci/on-strix-beta.sh`, which agents also use. A PreToolUse hook (`ci/production-guard.sh`) refuses `cargo build`/`test`/`check`/`clippy`/`run`/`bench`, `cargo tauri build`, `ci/ratchet.sh clippy` and `ci/acceptance.sh` on that host, unless they go through `ci/on-strix-beta.sh`. The release job builds on a Strix runner too. The server keeps only signing, publishing and installing (see `2026-11-12-release-builds-on-strix.md`).

**Reason.** The owner's rule is that the production server runs no tests. On 2026-11-11 it was broken twice:
- About 19:40–20:10: agents ran `cargo test --workspace` and clippy locally while the release job built the workspace.
- About 22:27–22:48: an agent ran the Firecracker invariant suites locally.

Both times memory pressure followed, and the Wi-Fi card (MT7922, `mt7921e`) stopped receiving packets without logging an error. The firewall lets SSH in only over Wi-Fi and Tailscale, so the headless server was unreachable and had to be power-cycled at 20:32 and at 04:06.

The rule had lived only in one session's memory, which subagents never see. Meanwhile `AGENTS.md` told every agent to run `cargo test` against a local Postgres. A rule in `AGENTS.md` alone would still depend on every agent reading it, so the hook enforces it on the host.
