# The owner's specs against Magnus today: what needs your decision

Status: ruled in part (below); the rest are questions for the owner, each with a proposed answer. Sources: the
owner's "Magnus Connectors Spec" (rev 17), "Magnus Admin Console Spec"
(rev 12) and "Magnus Authentication & Isolation Design" (rev 6), all of
2026-10-31, read against master on 2026-11-01. "DEC n" is a line of
DECISIONS.md as of that date; plans already written are `auth.md` and
`admin.md` beside this file.

**Where things stand (2026-11-01).** The specs are built as written (owner:
"all specs are for implementation not proposal"), amended by the rulings
below. The owner ruled the first three groups; 15-22 are not yet ruled.
Built since: per-person connections (#62), the Library's connector cards and
the real catalog (#63), the connectors as a catalog of apps with no forms
(#67), and the admin console's own process and listener
(`admin-listener`). The relay, the rest of the launch catalog and the
console's sections follow, each its own pull request. The four marked
**invariant** still need the owner's approval to change a protected test.

## The owner's rulings (2026-11-01)

The owner answered the first three groups; the smaller conflicts (15-22) and
the open points are not yet ruled.

| # | Ruling |
| --- | --- |
| 1 | **No passkeys.** The browser-with-passkeys sign-in is not adopted: people go on signing in as today, with their username and password typed in the app (DEC 274, 276 stand), and nothing in Magnus asks for a passkey. |
| 2 | Approved, without passkeys: the backup password stays as the way back in when the provider is down, on this machine, never for the admin console. The admin signs in, and steps up, with their password (and their code when they use one) instead of a passkey. |
| 3-6 | Approved as proposed. |
| 7, 8, 10 | Approved as proposed. |
| 9 | **Approved** (owner, after the explanation below): per-person walls in the database, on the content store, with the start-up reading through its own role. Its own plan first, then built after 11-14. |
| 11-13 | Approved as proposed. |
| 14 | **A policy, not a fixed rule.** In an organisation, a person's data is the organisation's data, with no right to privacy from it: by default the admin can read every person's tasks, chats, files and memory, connector results included. A policy setting can make people's data private from the admin. Each read is still recorded. |

### 9, explained: walls between people's data in the database

Today the walls between people are in Magnus's own code: every request is
checked against who is asking (each person's things are their own, #33),
and the tests try to cross them. The database holds one log of everything,
which Magnus reads in full at start to rebuild what it knows.

The specs ask for a second wall inside the database itself (Postgres
row-level security keyed on the person), so that even a bug in Magnus's
code could not hand one person another's rows. The whole log cannot be
walled that way, because Magnus must read all of it at start. What can be
walled is the content store, where the words people write, their files and
their memory already live apart from the log since deletion step 1: the
connection that serves a person would see only that person's content, and
the start-up reading would use a separate role that sees everything.

Proposed then: not now. With ruling 14, a person's data is the organisation's
by default, so the database wall would guard mainly against a bug in
Magnus's checks, which the tests already cover. Build it, as its own plan,
when an organisation turns on private data. The owner approved building
the walls.

## Decisions that change recorded ones

| # | Question | The specs say | Today | Proposed |
| --- | --- | --- | --- | --- |
| 1 | How does a person sign in to the app? | The system browser, passkeys first; the app never collects a password (auth §2). | The app takes the username and password itself (DEC 274, 276; "just like logging into muse or grok bot", 2026-10-30), with the browser as fallback. | Follow the spec: the browser with passkeys, and record that it supersedes DEC 274 and 276. The typed password goes. |
| 2 | Does the owner keep the backup password? | The admin signs in with a passkey only; Magnus keeps no passwords (admin test 6, auth §5). | The owner's backup password with a one-time machine code works when the provider is down (DEC 278, 339). | Keep it only as the way back in when the provider is down, on this machine, and never for the admin console. |
| 3 | Who is a person? | The token's `sub` (auth §3). | The `preferred_username` claim (`config.rs:166`). A Keycloak account deleted and made again, or renamed, under the same username becomes the old person, with their tasks, memory and approvals. | Bind each person to their `sub` at their next sign-in and refuse a different `sub` after that. Existing people keep their records. |
| 4 | Which roles are there? | `admin` and `member` (auth §2). | `owner`, `approver:<risk>`, `policy:*`, `developer`, `tester`, `knowledge:endorse`; the invariants rely on `owner` and `approver:*`. | Map Keycloak's `admin` to `owner` and `member` to a person; keep the other roles as Magnus's own, set in Settings or by group (accounts.md step 4). |
| 5 | Can a person relax an approval the organisation set? | "A user or admin can relax" the default for local code tools (connectors). | Lower layers only narrow (DEC 28; invariants 6 and 8). | No: a person can only tighten. The admin relaxes for everyone. |
| 6 | Does installing an appliance update need the admin's step-up? | Yes (admin, Operations > Updates). | Updates install by themselves, with rollback (DEC 177-180, 365). | Keep automatic updates; step-up only for a manual install or a rollback. |

## Decisions the invariants depend on

| # | Question | The specs say | Today | Proposed |
| --- | --- | --- | --- | --- |
| 7 | **invariant** Who classifies a connector's tools? | A catalog's annotations set each tool's class, and a changed tool list is followed (connectors). | The owner classifies every tool before it runs, and a changed definition is disabled until re-approved (invariant 1: `tools_the_owner_has_not_classified_cannot_run`, `a_changed_tool_definition_is_disabled_and_hidden`; DEC 36, 102, 172). | A signed catalog manifest's classes count as the owner's classification; a changed definition arrives as a new manifest version, which the owner approves. The invariant tests stay as they are. |
| 8 | **invariant** Can the user API keep the owner's controls? | Owner routes and long-lived config tokens leave the user API for the admin console (admin). | The kill switch, secrets and policy are driven through the user API, as invariants 1, 2 and 6 do. | Build the console beside them first; move each control only with its invariant test moved too, with your approval each time. |
| 9 | **invariant** Per-person row-level security, and the log rebuilt at start | Row-level security on `sub`; a cross-person read returns nothing (auth §4, admin test 10). | The kernel rebuilds all state by reading every event at start (DEC 20). | Row-level security on the per-person content store only (what a person wrote and what was written for them), with replay reading through its own role. An architecture change: its own plan first. |
| 10 | **invariant** What does the kill switch do from the console? | Halt, suspend, pause VMs; release needs step-up (admin). Silent on the vault and sign-ins. | It also seals the vault and revokes every OAuth sign-in (DEC 86, 308). | The console's switch is the same switch: it seals and revokes as today, and release needs step-up. |

## Decisions on behaviour

| # | Question | Today | Proposed |
| --- | --- | --- | --- |
| 11 | New work while the kill switch is on | A task or chat turn is accepted, its run suspended at once; on release its person gets "Stopped by the kill switch" with Continue (DEC 336). The admin spec says new runs are refused. | Refuse: the person is told the kill switch is on, and nothing is queued. |
| 12 | A person turned off in Keycloak | Their sessions end within 10 minutes (DEC 341), but their schedules go on running for them. | Their schedules pause, and resume only when they are turned back on and resume them. The check covers everyone with a schedule, every 5 minutes, and records each session it ends. |
| 13 | Long-lived config tokens | Accepted on every listener, the LAN's HTTPS port included. | Only on the plain loopback listener, where the owner's scripts and the tests use them. |
| 14 | Can the admin read a person's private data? | The admin spec gives root a Data tab over tasks, chats, files and memory; the connectors spec says admins see only a connection's metadata, never what it returns. | One rule: root reads a person's data only with step-up, each read recorded and shown to the person; connector results are part of that data. |

## Smaller conflicts

| # | Question | Proposed |
| --- | --- | --- |
| 15 | The auth design bans mTLS and device certificates; the connectors relay uses mTLS; accounts.md plans device certificates. | The ban is for people's devices; the relay keeps mTLS; accounts.md's device-certificate step is dropped. |
| 16 | "One public client" beside the console's confidential client, its service account, and the status client. | Name them all: the app's public client, the console's confidential client, and service accounts for the console and the status check. |
| 17 | One `magnus.` and one `auth.pyrallon.ai` for many organisations' appliances. | Each appliance gets its own name under the organisation (`<org>.pyrallon.ai`, `auth.<org>.pyrallon.ai`); the registry that maps them needs a plan. |
| 18 | Org-level connectors in the console; none in the connectors spec. | Leave them out until the connectors spec defines one. |
| 19 | A persistent VM per person (auth §4) against a VM per run (DEC 109, 43). | A VM per run, never shared across people, as today. |
| 20 | Certificate pinning (DEC 111, 121) behind Cloudflare, which terminates TLS. | Behind the tunnel the apps trust Cloudflare's public certificate; pinning stays for the LAN address. Say in the trust model that Cloudflare sees the traffic. |
| 21 | "Done means … on Windows and Linux" (connectors) against no Windows tests (DEC 167). | Linux tested, Windows by the owner's recorded run, as for the app. |
| 22 | "Library" (files today; connectors in the spec) and "module" (removable run modules; installed packages; connectors). | "Library" stays the files; connectors are the "connector catalog"; "module" stays the removable run modules. |

## Not yet specified

- The appliance registry and the hostname of the connect flow's completion endpoint: who runs it, and how the relay knows an appliance.
- The "curator service identity" (auth §4): the memory pass, the knowledge extractor, or a new agent?
- The console's Context and Ontology: what they hold, and how agents receive them under pinned instructions.
- Which ACR value means "passkey" for step-up, and Keycloak's mapping for it.
- A per-VM egress proxy for brokered connectors.
- How an admin-added server's tools are approved once nobody classifies them by hand (see 7).

## Order once ruled

Known bugs first (the owner race is done, #50), then, each its own pull
request: identity by `sub` (3), the kill switch refusing new work (11),
schedules of a person turned off (12), config tokens on loopback (13), the
admin's access to people's data as a policy (14); then the authentication
foundations without passkeys, the admin console, and the connectors, as
`auth.md` and `admin.md` order them, amended by these rulings.
