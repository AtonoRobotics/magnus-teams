# The deep review's remaining gaps: what is decided, built, or planned

The owner, 2026-11-13: "Close the all gaps. Recommend solutions." This file
covers each gap from the deep review that is not fixed by a commit of its
own. Gaps that are being built have their own decision file.

## Decided here

**GAP4, payments (Link one-time card; US and Canada only).** The owner chose
on 2026-11-09 that Link's one-time card comes first, with the vault card as
the fallback behind its own one-time approval
(`2026-11-09-payments-link-wallet.md`). That stands. Link serving only the US
and Canada needs no new rule: a person Link cannot serve is a person for whom
"Link cannot serve the person at all", so they get the saved-card path, with
an approval every time. Recommended spec wording, replacing "Saved cards
never autofill": "A saved card never fills without its own one-time approval
for that purchase; policy cannot pre-approve it." Payments stay built last.

**GAP8, retention.** Live chats and files are kept until their person
deletes them, or deletes their account. There is no automatic expiry. An
expiry would delete people's work without their asking, which is a promise
about their data that the spec does not make, and the safer reading is not
to make it. The trash and backup windows stay as decided
(`2026-11-11-retention-is-set-within-the-purge-window.md`). The review said
the 30-day purge against the append-only log "rests on convention". It does
not: content is stored apart from the log's record
(`log_content_tests.rs` `content_leaves_the_record_and_joins_back_as_written`),
and inv3 `a_deleted_file_is_gone_at_once_and_nothing_outlives_the_purge_window`
proves the window.

**Short secrets (surrogate `MIN_ECHO_BYTES` = 8, vault `redact` under 4
bytes).** Accepted, with the reason recorded. Scrubbing a 3-byte or 7-byte
string out of everything the model sees would rewrite ordinary words and
numbers, and the rewritten text would tell the model where a credential sits.
A credential that short gives no protection anyway. What protects it is that
credentials go from the vault to the page or the provider and never to the
model (invariant 2), not the scrubber, which is a second line.

**Admin and console text (GAP1, GAP2).** Done in `f2bf39b`. The proposals
under `docs/proposals/` are dated proposals and stay as written; the
decisions supersede them.

## Being built now (own decision files when they land)

- GAP5, Outlook read-back: send through a draft so the message has an id,
  and read it back from Sent Items.
- GAP7, catalog trust: manifests stay compiled into the signed release; a
  signed withdrawal list beside the update feed disables a connector between
  releases, revokes its connections and tells its users.
- GAP9, takeover leftovers: processes started during a takeover are stopped
  at handback; if the computer cannot confirm that, the agent stays paused.
- CI and test hygiene: contract tests fail in CI when their credentials are
  missing, instead of passing empty; the production guard covers `eval`,
  `xargs`, `make` and pipes into a shell; brokered connections pin their DNS
  answer and refuse private addresses; the fast tier's timeout matches its
  real duration.
- The intermittent dropped guest connection after a cold boot, which fails
  three invariants at random.

## Planned, in this order (each a build of its own)

**GAP6, the relay handoff.** Confidential clients (Google, Microsoft, Slack)
need a client secret, which today an admin types into Admin › Connectors. The
relay on pyrallon.ai should hold it: the server starts a sign-in through the
relay, the relay completes the code exchange with its secret and hands the
tokens to the server over the server's pinned channel, signed with a
per-server key the relay issued at registration; the relay keeps no tokens.
Refresh and revocation go through the same channel. Until it is built, the
admin-typed secret stays, and connections keep working while the relay is
unreachable until their access tokens expire. Order: the signed handoff
message format and its verification on the server first (testable with a
fake relay), then the relay service, then moving each provider over.

**GAP10, local desktop programs.** Built in phases, each one useful alone:
1. The bridge in the desktop app: detect supported programs, one consent per
   program, a loopback-only tunnel over the app's existing channel to the
   server, and every local call through the server's Cedar policy, with
   code-executing tools asking by default.
2. Blender first (an official MCP server and add-on, Windows and Linux),
   then FreeCAD (our workbench, Windows and Linux).
3. Fusion (its built-in MCP server, Windows; a setting, not an install).
4. SolidWorks last (our COM add-in, Windows, licensed and single-session).
Phase 1 is the large one; the programs after it are small.

**The release has no acceptance gate.** Every push to `main` releases to the
test channel by design. The heavy suites (installed app, injection, update,
full invariants) should gate the promotion from test to development, not
the push: run `ci/acceptance.sh` on Strix against the candidate, and promote
only on green.

**Weak oracles in the invariants** (`never()` windows of a few seconds;
`PATIENCE` of 90 s): a late violation can be missed. The fix is to make each
check event-driven (wait for the run to end, then assert over the whole
record) rather than lengthening windows. It is an invariant edit, done with
the owner's approval when taken up.
