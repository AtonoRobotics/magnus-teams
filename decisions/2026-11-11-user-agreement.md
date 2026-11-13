# The user agreement: Terms of Use and Privacy notice, on a shared-environment model

The owner, 2026-11-11: "Keep admin sharing of team data but include in user
agreement that using teams is a shared environment. Research and use relevant
user agreements that are open source." Later the same day the owner changed
the model the agreement describes: "Create Team Admin group. Magnus admin
provides support for Magnus user accounts, server, and server related things.
Users are isolated and private parties unless they agree to other agreements
or share in a team workspace. Anything put in a team workspace is public to
that team. The team should be isolated from other teams. Team Admin owns the
workspace."

Choice: the Terms of Use and Privacy notice (`crates/magnus/src/legal/documents.rs`)
state that model:

- Outside a team workspace each person is a private party: their data is
  theirs alone unless they accept another agreement (the Tester Terms) or
  share it in a team workspace.
- A team workspace is a shared environment: anything posted or shared there
  is visible to its members; its Team Admin owns it and can moderate and
  delete what is in it; sharing copies the item and the person keeps their
  original; teams are isolated from each other.
- The Magnus admin supports accounts and runs the server (invites, password
  resets, turning accounts off, removing people, server settings, the
  organisation's connectors, and pausing a computer or stopping a task for the
  server's safety) and does not see a person's data or any team's content;
  support that needs a person's data happens only in a session they grant.
  The powers that contradict this (an admin reading a person's data with a
  log entry; a server admin adding themselves to a workspace) are being
  removed from the code by a separate change the same day, as the owner
  directed; the text does not describe them.
- Developers see nothing by default, anonymous usage data with a yes, full
  data for testers (`2026-11-11-anonymous-developer-data.md`).

Each person accepts the Terms of Use, with the Privacy notice shown beside
them, before the app opens; existing people are asked at their next sign-in.
The only other answer is signing out. Acceptance is `legal` event
`terms_accepted` with both versions. A version is the text's date and a
digest of the text as this server renders it, so a change to the text or to
the `[legal]` facts it names asks everyone again. The app enforces the gate;
the API records it. Scripts using a config person's API token are the
owner's own and are not gated.

The documents are pages in the app (Settings › Agreements, and the gate) and
public pages on the server without signing in: `/legal/terms`,
`/legal/privacy`, `/legal/testers`.

## Sources

Base: Basecamp's open policies, github.com/basecamp/policies (terms and
privacy), © 37signals LLC, under Creative Commons Attribution 4.0 International
(verified in the repository's LICENSE.md and README on 2026-11-11). Each
document carries an attribution line saying so and that it was changed.
Automattic's legalmattic (CC BY-SA 4.0) was not used: share-alike would bind
this text to that license. No openly licensed tester agreement fit; the Tester
Terms are new text on the Basecamp base.

From Basecamp the documents take their plain voice and structure (account
terms, "what you put in stays yours", "as is" and "as available", limitation
of liability, changes; "we keep only what we need", who sees your content,
your rights, retention, security). They drop what Magnus does not do
(payments, refunds, plans, uptime promises, EU transfer clauses) and state
only what the code keeps: retention follows `2026-11-11-retention-is-set-within-the-purge-window.md`
(trash at most 30 days, then out of the backups within 30 days; the log's
usage records kept while the server runs), deletion requests follow Settings
(the owner has 30 days), secrets are in the vault and passwords stored as
argon2 hashes.

## Config: `[legal]`, required

Facts the code cannot know come from typed config, checked at startup
(`legal::Settings::check`, called by `Config::load`); a missing or empty one
stops the server with a message naming each and the line to add:

- `operator`: the legal name of whoever runs the server.
- `contact`: the address people write to (an email; for this server
  `magnus@pyrallon.ai`).
- `governing_law`: the law the Terms are under.
- `tester_copy_days`: the most days developers keep what they copy from a
  tester's data.

The invariants harness writes its own config and so needs the section too:
`decisions/2026-11-11-invariants-legal-section.patch` (`git apply` it; edits
to `tests/invariants/` wait for the owner).

## The invariants' `[legal]` patch: not applied (2026-11-13)

`2026-11-11-invariants-legal-section.patch` was written when `[legal]` was required and a server without it would not start. Since `3cc8780` a server with no `[legal]` section starts with the agreements off; only a partial section is refused (`Config::load`). The invariants harness writes no section, so its server starts. No invariant tests the agreements, and the API records acceptance without gating on it, so the section changes nothing they check. The production server (`~/.magnus-teams/magnus.toml`) has no `[legal]` either, so the harness runs the configuration production runs. The patch was removed unapplied. When the agreements are switched on in production, the harness should get the section too.
