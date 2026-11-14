# CI and test hygiene: honest contract skips, a harder production guard, a pinned egress dial, a real fast-tier timeout

## Contract tests say when they skip, and fail where they are required

**Choice.** `gmail_contract.rs`, `calendar_contract.rs`, `slack_contract.rs` and `google_chat_contract.rs` used to print a line and return when the owner's test-account credentials were missing, which libtest reports as a pass, and a passing test's output is captured, so nobody saw the line. They now ask `magnus_scratch::contract::credentials` first, before a microVM boots:

- with `CI` (GitHub Actions sets `CI=true`) or `MAGNUS_REQUIRE_CONTRACTS` set to anything but empty, `0` or `false`, a missing or empty credential fails the test, naming the variables that are missing;
- otherwise the test still runs its no-account half (the provider reached from the microVM, an unknown token said as a sign-in to do again) and skips the account half with `SKIPPED: <reason>`, written to the process's own stderr, which libtest does not capture, so the skip shows even though the test is reported `ok`.

`ci/acceptance.sh contracts` runs the four with `MAGNUS_REQUIRE_CONTRACTS=1`. It is a named suite, not in the default list, until the owner's test accounts are given to Strix Beta (`docs/connectors.md` still lists that as open); once they are, it joins the default list and a release cannot ship on a skipped contract.

**Why not `#[ignore]` with `--ignored`.** Each test has two halves that share one image build and one microVM: the no-account half needs no credentials and is worth running on every run. `#[ignore]` works per test function, so it would mean splitting each test in two and building and booting twice, and an ignored test is then not run at all by the plain `cargo test -p magnus-scratch --test gmail_contract` people use today. An `--ignored` run also cannot tell "credentials missing here, fine" from "credentials missing in the release run", which is the distinction this needs. The environment rule keeps one test, makes a local skip visible, and makes the required case fail.

## The production guard reads more of what a command runs

**Choice.** `ci/production-guard.sh` also refuses, on the production server:

- a shell reading its stdin from a pipe (`sh`, `bash -s`, `zsh -`, ...): the text `echo` or `printf` feeds it is split and checked like a command (each word on its own line too, and `\n` as a line break, so `printf '%s\n' 'cargo build' | sh` is caught), and so is a here-document `cat` feeds it; fed by anything else (`curl ... | sh`, `cat build.sh | bash`) it is refused, because the script cannot be read;
- `eval ARGS`: its arguments, joined, are split and checked as a command;
- what `echo`/`printf` prints inside `$(...)` or backquotes, which a shell may run as a command (`x=$(echo cargo build); $x`);
- cargo with a compiling subcommand wherever the words put it: after `sudo`, `nohup`, `doas`, `builtin` and the earlier prefixes, after cargo's own options (`cargo --locked build`), as the command of `xargs` or `parallel` (refused also when it has no subcommand, since its input supplies one: `echo test | xargs cargo`), and among the words of `find -exec`, `watch`, `flock`, `taskset`, `strace` and similar runners;
- `make` and `just` run with no target (the default target, which usually builds) or with a target named like a build or test: `build`, `test(s)`, `check`, `clippy`, `all`, `release`, `bench`, `ci`, `lint`, `install`, `acceptance`, `invariants`, alone or as a `-`/`_`/`:` separated part (`check-all`). Their help and listings (`make -h`, `just --list`) pass. This repository has no top-level Makefile or justfile, so reading recipes would guard nothing today; naming is the simple rule that holds when one appears, and a recipe named otherwise that builds is the case it misses.

Each has deny and allow cases in `ci/production-guard-check.sh` (run by the fast tier).

**Not covered: scripts.** `./something.sh` or `sh something.sh` runs whatever the file says, and reading the file would refuse `ci/ratchet.sh size` because the same script has a `clippy` branch. A static guard cannot judge a script without running it; the guard is a seat belt against accidents, not a barrier against someone set on building here, and the two known heavy scripts (`ci/acceptance.sh`, `ci/ratchet.sh clippy`) stay refused by name.

## The brokered egress proxy dials an address it checked

**Choice.** The microVM's egress proxy (`brokered.rs`, `tunnel`) used to `TcpStream::connect` the manifest host by name, so a provider name that DNS pointed at a private, loopback, link-local or unique-local address reached the server's own network. It now dials through `dial`: a host the config pins in `[brokered] resolve` (an operator's internal or pinned address, or a test's stand-in) is reached at that address; any other is resolved once by `web::public_addrs`, refused if any answer is private (`web::private_ip`, IPv4-mapped IPv6 included), and connected to the addresses that were checked, so DNS cannot answer differently between the check and the connection. `web.rs`'s fetch uses the same helper. A refusal answers the microVM `502` and logs why.

**Why `resolve` is the internal list.** It is already the config's way to say where a manifest host is reached, documented as "a pinned address, or a stand-in"; an operator who means a provider to be internal says so there. The hosts a microVM may ask for remain only its manifest's.

## The fast tier's timeout is ten minutes

**Choice.** `fast.yml` said "under five minutes" and set `timeout-minutes: 30`. Measured on 2026-11-13 from the last 100 runs: 67 successes, median 249 s, 90th percentile 331 s, longest 402 s; in recent warm runs `cargo test --workspace --lib --bins` takes about 205 s and `ci/ratchet.sh clippy` about 35 s, everything else under 20 s each. The header now states the target as five minutes warm with those numbers, and the timeout is 10 minutes, about twice the typical run and above the longest.

**Not done: sharding.** Typical runs sit at the five-minute line. If they drift over it, the cheapest step is running clippy as a second job beside the tests (about 35 s off the wall clock, at the cost of a second runner's minutes and cache); splitting `cargo test` itself across jobs gains less, since each shard compiles the workspace again.
