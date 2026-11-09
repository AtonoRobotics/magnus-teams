# Magnus: an autonomous digital worker that runs on a server and is used through a desktop app

## Read first
- `decisions/`, newest last: every choice since 2026-11-06 with its reason. `DECISIONS.md` and `PROGRESS.md` are the record before that.
- `docs/`: connectors, the admin console, remote access, proposals and research. The product spec is the Magnus Agent Computer spec; where a document here disagrees with it, the spec wins.
- `tests/invariants/`: the eight invariants as tests. `.claude/settings.json` denies edits to them. A change needs the owner.

## How work lands
Commit small changes to `main`. A pull request is optional. The fast tier (`.github/workflows/fast.yml`) runs on every push and stays under five minutes. Heavy suites (installed app, injection, update, full invariants) run as acceptance against a release candidate, never as a gate on a push.

Record each new decision as a file in `decisions/` named `<date>-<slug>.md`: the choice and its reason. Commit working states and revert a failed attempt instead of patching it. A fix comes with a test that fails without it.

## Commands
- `cargo fmt --all --check`
- `ci/ratchet.sh size` and `ci/ratchet.sh clippy`
- `cargo test --workspace --lib --bins` (needs Postgres on 127.0.0.1:54340, or `MAGNUS_TEST_PG`)

## Rules
- A duration or limit is a named constant with its unit in the name. No `Duration::from_*` with a bare number outside the constants and tests.
- Config is typed, validated at startup and passed in. Logic never reads the environment.
- No `unwrap()` or `expect()` outside tests. Clippy warns on them, and the count may only fall (`ci/clippy-baseline`).
- A source file stays under 800 lines. Files over it are listed in `ci/oversize.txt` and may only shrink. New code goes in a new module.
- Secrets come from the vault only.
- The wire types are defined once in Rust. The UI is plain JavaScript with no build step, and its types come from JSDoc.
- An event kind is never edited or deleted. A retired kind keeps a reader that translates it to the current form.

The reasons for each rule are in the Magnus Code Standard.

## Layout
`crates/magnus` the kernel, `crates/adapter-*` model providers, `sandbox/` the VM guest and image, `brokered/` connector servers, `desktop/` the app and admin console, `catalog/` connector entries, `tests/` the invariants and the rest.
