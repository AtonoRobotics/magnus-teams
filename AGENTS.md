# Magnus: an autonomous digital worker that runs on a server and is used through a desktop app

## Read first
- `decisions/`, newest last: every choice since 2026-11-06 with its reason. `DECISIONS.md` and `PROGRESS.md` are the record before that.
- `docs/`: connectors, the retired admin console, remote access, proposals and research. The product spec is the Magnus Agent Computer spec; where a document here disagrees with it, the spec wins.
- `tests/invariants/`: the eight invariants as tests. `.claude/settings.json` denies edits to them. A change needs the owner.

## How work lands
Commit small changes to `main`. A pull request is optional. The fast tier (`.github/workflows/fast.yml`) runs on every push and stays under five minutes. Heavy suites (installed app, injection, update, full invariants) run as acceptance against a release candidate, never as a gate on a push.

Record each new decision as a file in `decisions/` named `<date>-<slug>.md`: the choice and its reason. Commit working states and revert a failed attempt instead of patching it. A fix comes with a test that fails without it.

## Deciding
Decide anything that follows from the spec, the decisions and the Code Standard, record it in `decisions/` with its reason, and keep working. That covers design choices with a clear best option, wording, pushing your own commits, accepting spec suggestions that carry a decision already made, and fixing a spec contradiction toward the safer reading.

Stop and ask the owner only for:
- a step you cannot take yourself: a privilege, a host or production change, an edit to `tests/invariants/` (send a `git apply` patch);
- a new promise to users that the spec does not make, about their data, money or privacy;
- a choice between options that trade those against each other, with no rule settling it.

When you must ask, ask once with your recommendation, and keep working on what does not depend on the answer.

## Where builds and tests run
`magnus-server` is the production server. Nothing is built or tested on it: no `cargo build`, `test`, `check` or `clippy`, no `cargo tauri build`, no invariant or scratch suite, no test VM. Every build and test runs on Strix Beta through `ci/on-strix-beta.sh NAME COMMAND...`, which copies the checkout there and runs the command. Use a NAME of your own for each change. A hook (`ci/production-guard.sh`) refuses these commands on the server. When you brief a subagent, repeat this rule in the brief.

## Commands
- `cargo fmt --all --check` and `ci/ratchet.sh size` (light, fine anywhere)
- `ci/on-strix-beta.sh NAME ci/ratchet.sh clippy`
- `ci/on-strix-beta.sh NAME cargo test --workspace --lib --bins` (Strix Beta has the test Postgres)

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
`crates/magnus` the kernel, `crates/adapter-*` model providers, `sandbox/` the VM guest and image, `brokered/` connector servers, `desktop/` the app, its Admin section included, `catalog/` connector entries, `tests/` the invariants and the rest.
