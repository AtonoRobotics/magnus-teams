# Developer views are anonymous, and show nothing of anyone who did not say yes

The spec said "the developer side sees everything", and every developer route
(`/v1/dev/runs`, the run inspector, the raw log, experiments, metrics, policy,
playbook, modules, computers and their egress, tool screens) showed every
person's replies, tool inputs and results, memory and playbook. A 2026-11-08
decision had already kept screens and files behind a session the user grants.

The owner settled it on 2026-11-11: "share full data with developer [for
testers] … Make developer data anonymous."

Choice: one module, `crates/magnus/src/anonymize.rs`, decides what every
developer route shows of each person, through a `Lens` built for the viewing
developer:

- **Full**: the developer's own work, and a tester's (Tester Terms accepted,
  at the version that reads now). Shown as before, with #257's redaction of
  another person's relationship notes kept.
- **Anonymous**: someone whose answer to the usage-data question, at the
  version that reads now, is yes. Shown through `shape`, an allowlist walk:
  a string survives only under a key named in `CODE_KEYS` and only when it is
  a bare token (no spaces, no `@`, no `/`, no `://`); ids under
  `PSEUDONYM_KEYS` become `anon-<12 hex>` of an HMAC-SHA256 keyed by a 32-byte
  secret made on first use and kept in the vault (`developer-view/pseudonym-key`),
  so the same person has the same pseudonym on this server and no developer
  can turn it back; whole content fields (`input`, `body`, `blocks`, `details`,
  `cedar` …) go. Numbers stay. Egress rows keep the host and lose path, query,
  address and detail.
- **Hidden**: everyone else, those who said no and those not yet asked. Their
  runs answer 404 and are left out of every list, and of every count: metrics,
  module tallies, reliability and the measured panels are computed over the
  runs the viewer may see (`dev_panels::runs_seen`), and any hidden run id left
  in an answer is scrubbed (`anonymize::scrub`). Only the capture-overhead
  panel, which holds no per-person data, counts the whole server.

A workspace's computer is shown only as far as every member allows. A field
added to a developer view later is withheld from anonymous rows until someone
names it in `CODE_KEYS`: the safe direction for a new field.

Not yet asked counts as no, because sharing is a promise about their data
that they have not made. An answer to an older text of the question counts as
not yet asked.

Why one module: the rule is checked in one place
(`anonymize::tests`) and end to end across every developer route
(`legal::tests::developer_views_show_testers_in_full_sharers_anonymised_and_nobody_else`),
which plants words, an email address, a URL path and query and a file name in
the runs of a sharer, a refuser, a tester and someone never asked, and fails
if any route shows them.

`dev.rs` moved its metrics, policy and playbook handlers to `dev_panels.rs`
to stay under its size baseline; the baseline falls to its new length.

Heavy suites that read another person's runs as a developer
(`tests/scratch`: `ask_reasoning`, the modules, reviewer and chat experiments)
now need that person to have joined the tester program, or the run to be the
developer's own. They run as acceptance against a release candidate, not on a
push; bringing them over (each such test's person joins the program through
`/v1/legal/testers/join` before the developer reads) is left open, as is the
invariants harness's `[legal]` section (`2026-11-11-user-agreement.md`).
