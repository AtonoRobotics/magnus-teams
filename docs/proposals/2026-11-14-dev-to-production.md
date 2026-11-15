# Magnus: from dev to production

A review of how Magnus runs today and what it offers. It covers accounts, settings, security, UI/UX, teams and autonomy, then gives a staged plan to a production release. It is checked against spec rev 148, the decisions through 2026-11-14, the code at `fec4ce8`, and current best practice (sources at the end).

The owner's rules hold throughout. A Magnus admin is support-only and reads no one's data. People are private. A team's workspace is shared within the team and isolated from other teams. The user-facing word is "server".

Each item is tagged with a priority:
- **P0**: before anyone outside the owner relies on it.
- **P1**: needed for the production release.
- **P2**: after the release.

---

## 1. Where Magnus stands

The product core is strong and unusually well guarded:
- an append-only event log;
- per-person sandboxed computers;
- policy layers with a kernel floor;
- asks with "See exactly what it would do";
- receipts and evidence;
- a four-level kill switch;
- erasure that reaches backups;
- strong accessibility basics;
- eight invariants as tests.

The architecture work on 2026-11-14 left one place each for authorization, tools, policy, conversations and state folds.

What stands between this and a production release is mostly **operations and release discipline**, plus a handful of **account and autonomy features people will expect on day one**:

| Area | State |
|---|---|
| Release gating | **Every push to `main` installs onto the production server within about 5 minutes.** No fast-tier or acceptance gate. Invariants were last at 120/123. |
| Host | One mini-PC with no HA. Two outages on 2026-11-11 needed a power cycle, caused by Wi-Fi RX wedging under memory pressure. The fixes are not applied. |
| Data | One daily encrypted backup, on the same disk as its key. No off-site copy, no point-in-time restore, no production restore drill. Backups grew from 22 KB to 160 MB in four days. |
| Monitoring | Nothing outside the app watches the Magnus server. The host health check does not probe it. |
| Legal | `[legal]` is unset in production, so the agreements are off. The public terms and privacy pages return 404. |
| Accounts | Two-factor can only be enrolled from the server CLI. A time zone is stored but no screen sets it. Account deletion is an admin-approved request. |
| Teams | A Magnus admin removing a person can put someone of their choosing into a team as Owner. |

---

## 2. P0 fixes: real defects found in this review

All of these follow from the spec and the owner's rules.

1. **Removing a person must not choose who joins a team.**
   - The problem: `admin_app/remove.rs:57-75,134-141` and `admin-people.js:72-79` let the admin pick any person on the server. That person is then added to the leaver's workspaces as Owner. Even "delete" passes owned workspaces on.
   - Fix: hand ownership only to the workspace's existing Team Admins or members, picked from the workspace's own member list. If no member remains, the workspace goes with the person, as it would on deletion.
   - Test: the admin's choice can never add a non-member.
2. **Two-factor must not lock people out.**
   - The problem: with "Everyone uses a code" on, anyone not enrolled is refused as a wrong password (`accounts/check.rs:436-446`). Enrollment exists only in `cli.rs:349`.
   - Fix: self-service enrollment in Settings › Account (QR code, confirm one code, then show one-time recovery codes).
   - At sign-in, a person who must have a code and has none is taken through enrollment, not refused.
   - The admin policy screen warns how many people are not yet enrolled.
3. **Stop the browser UI being framed.** Add `frame-ancestors 'none'` to the CSP and `X-Frame-Options: DENY` (`ui/assets.rs`). Add HSTS when TLS is on.
4. **Native `confirm()` in six places** can silently answer "no" in the Windows webview. These are update/rollback, turn a person off, the sign-in policy, and deleting computer files (`admin-ops.js:72-73`, `admin-people.js:139`, `admin-security.js:166`, `computers.js:118,454`). Use `ask()`. Add a node check that bans `confirm(`, `alert(` and `prompt(` in `desktop/ui`.
5. **One time-zone fallback.**
   - The problem: with no zone stored, proactive work uses UTC (`proactive.rs:41`) while daily schedules use the server zone, and `/v1/preferences` reports "UTC".
   - Fix: every reader takes the zone from one function, with one documented fallback, the server's zone. Section 3.2 removes the need for the fallback.
6. **"appliance" in about 26 user-facing server strings**, for example `api.rs:212,441,521`, `oidc.rs:306,356,550`, `kernel.rs:202` and `ui/routes.rs:47`. Replace them with "server" and add a check that the word is absent from user-facing strings.
7. **Spec text that contradicts the owner's rules.** Rev 148 still says "Every admin read of a person's data is logged, and the person is told", "An admin sees what an agent did", and per-project spend for admins. The 2026-11-11 decision overrides these, but the spec edits never landed. Lay them in the doc as suggestions for the owner.

---

## 3. User accounts

### 3.1 Sign-in and recovery (NIST SP 800-63B-4, OWASP ASVS 5.0)

| Feature | Today | Recommendation | Priority |
|---|---|---|---|
| Password length | Policy checks exist | Minimum **15** characters for password-only accounts, **8** with MFA. Accept at least 64. No composition rules and no rotation. | P1 |
| Breached-password check | None | Refuse passwords found in a breach corpus, and say why. A self-hosted server should check a downloaded HIBP k-anonymity list locally, so nothing leaves the server. | P1 |
| Lockout | Counter, wait, permanent lock | Prefer throttling to a hard lock (a hard lock lets an attacker lock people out). NIST caps it at 100 consecutive failures. Keep the admin's limits, but default to an increasing wait. | P1 |
| TOTP | CLI only | Self-service, as in P0 #2, with one-time recovery codes. | P0 |
| Passkeys | Only through the identity provider's browser | Native **WebAuthn passkeys** in the app as the main sign-in, with password as a fallback. Allow several passkeys per person. A passkey satisfies "Everyone uses a code". | P1 |
| SSO | OIDC (Keycloak-style) | Keep it. Add **SCIM** deprovisioning for organisations: a deactivated person loses sessions, grants and running work at once. | P2 |
| Account recovery | Email reset (does nothing without mail) | Recovery codes, a second passkey, or an admin-issued reset link that never shows the admin anything of the person's. Never security questions. If mail is not configured, "Forgot password" tells the person to ask their admin. | P1 |
| Step-up | Admin changes only | Ask for a fresh sign-in before a person's own high-risk actions too: approving a payment, adding a connector, raising a limit or standing authority, exporting data, changing recovery or two-factor. | P1 |

### 3.2 Profile and preferences

| Feature | Recommendation | Priority |
|---|---|---|
| **Time zone** | Detect it on first sign-in with `Intl.DateTimeFormat().resolvedOptions().timeZone` and store the IANA name. Show it in Settings › Account with a searchable picker. When the device zone differs from the saved one, ask once ("You seem to be in Lisbon. Use Lisbon time for your schedules?"). Every time the app shows (schedule next run, quiet hours, card times) is drawn in the person's zone and labelled when it differs from the device. Tell the model the person's zone and current local time in its context, so "every day at 8" means their 8. | **P0** (done server-side 2026-11-14; UI missing) |
| Working hours and quiet hours | Stored, no UI. Add both to Settings › Notifications. During quiet hours, asks queue and nothing pings except what the person marks urgent. Unanswered asks never proceed; the task waits or ends safely, as today. | P1 |
| Display name and avatar | A name the person can set, defaulting to the identity provider's; an uploaded avatar, defaulting to initials. Both show in teams. | P1 |
| Locale and formats | Date, time, first day of week and 12/24-hour, all from the locale and overridable. Language is section 6.4. | P1 |
| Theme follows the person | Store appearance on the server so it follows the person to every device. The computer's desktop follows it (`desktop_theme` hook exists). | P2 |
| Notification channels | Keep the three server choices plus per-channel all/mentions/off. Add **per-kind** settings: asks, finished work, failures, team mentions, spend warnings. Asks and failures are on by default and cannot be fully silenced; quiet hours only defer them. | P1 |

### 3.3 Your data

| Feature | Recommendation | Priority |
|---|---|---|
| **Export everything** | One "Download my data" producing a machine-readable archive (GDPR Art. 20): chats, files, knowledge, memory, rules, schedules, grants, receipts and the computer's volume and manifest (the spec already says these "export as ordinary files"). Built in the background, with a notice when ready and a step-up to download. | P1 |
| **Delete my account** | Self-service, behind a step-up and a typed confirmation. Today it is a request the admin may decline and whose free text the admin reads, which conflicts with "a Magnus admin reads nothing". Use the existing erasure path. Offer an export first. Say plainly what goes and that backups are scrubbed within the stated window. The admin is told only that an account was deleted. | P1 |
| Connected apps | A page listing each connection with its scopes, last use and what the agent did with it. Revocation is one click and also revokes the token at the provider (RFC 7009). | P1 |
| Sessions and devices | Add device name, approximate place (from IP, kept only as long as the session), last seen, and "Sign out everywhere else". Notify on a new device. | P1 |
| Spend, for the person | A "Your usage" view showing this month against their limit, per task and per grant. Warnings at 75% and 90% go to the person, not only to admins. | P1 |

---

## 4. Application settings

**Shape.** Keep the decided split: *Settings* belongs to the person, *Admin* to the server, *the workspace* to its Team Admin. Fix what crosses it:
- **Mail setup** is shown inside Settings › Account. Move it to Admin › Operations. Make mail configurable in the app, not only in `magnus.toml`: the SMTP password goes to the vault, there is a test send, and the step-up applies.
- **Legal setup** (operator name, contact, governing law, the hosted terms and privacy URLs) becomes an Admin › Operations card, with a first-run check that refuses to open the server to people until it is filled in (see 8.4).
- **Search provider**: an Admin › Models card to choose one (none today). Until one is chosen the agent searches through the browser, and the card says so.
- **Model failover**: Admin › Models gets an ordered fallback list. When the default provider is unavailable, a run moves to the next one under the same policy, and the receipt says which model did the work.

**Remove or simplify:**
- The retired ask kinds `contract_change`, `review` and `policy_proposal` still have renderers (`app.js:321,335,1725,1776,1803`) although `legacy_items.rs` converts them at startup. Delete the renderers.
- The "The agent changed since you last looked" banner (`app.js:1181`) shows the person internal mechanics. Move it to the developer view, as "silence by default" asks.
- The welcome line says Magnus "proposes a schedule for you to confirm" (`app.js:1403`). That contradicts spec rev 140; reword it.

---

## 5. Security

### 5.1 Server and client (OWASP ASVS 5.0 Level 2 as the target)

| Item | Recommendation | Priority |
|---|---|---|
| ASVS L2 self-assessment | Walk the 17 chapters once, record the gaps as decisions, and re-check each release. | P1 |
| Request rate limits | Today only sign-in is limited. Add per-session and per-IP limits on the API, tighter on mail-sending, invites and exports. Return 429 with Retry-After. | P1 |
| Encryption at rest | The vault key sits beside the vault, and the backup key on the same disk as the backups. Keep the vault key outside the data directory (OS keyring or a TPM-sealed file on the host), and keep the **backup key off the server** (the owner's password manager, printed recovery). Encrypt the Postgres volume (LUKS) on the production host. | P1 |
| TLS | The public address goes through a Cloudflare tunnel whose connector is undocumented. Document where it runs. Serve a real certificate end to end, or keep the tunnel and bind 7800 to localhost only. Turn on HSTS. | P1 |
| Plaintext credential files | `postgres-admin.password`, `samuel-first-password` and `admin-url` sit beside the config. Move them into the vault, delete the first-password file once used, and add a startup warning while any remain. | P0 |
| Dependencies and supply chain | Add `cargo-deny` (advisories, licences, sources) and `cargo audit` to the fast tier. Produce a CycloneDX or SPDX SBOM per release. Add build provenance (SLSA L2, Sigstore signing) next to the existing release signature. Desktop builds are OS code-signed and notarized. | P1 |
| Vulnerability disclosure | Publish `/.well-known/security.txt` (RFC 9116) and a disclosure policy with safe-harbour language. | P1 |
| Tauri hardening | Check capabilities are deny-by-default, per window, with no wildcards. Keep the strict CSP in `tauri.conf.json`. Plan rotation of the updater key. | P1 |

### 5.2 Agent-specific security (OWASP Agentic Top 10 2026, LLM Top 10 2025, Anthropic computer-use guidance)

Magnus already does more than most here: content walls, taint, the policy floor and asks. To close it out:
- **Treat every screen, web page, mail and connector result as untrusted input.** Add the injection acceptance suite (`ci/acceptance.sh` has one) to the release gate, and grow it from the OWASP agentic threat list: goal hijack, tool misuse, memory poisoning, cascading failures. (P1)
- **Egress allowlists per computer.** The computer's network goes only to domains the task or the person's rules allow, and anything else is an ask. The spec's posture supports this; make it a visible per-person setting with safe defaults. (P1)
- **Signed, expiring approvals for irreversible actions**, payments above all. Each approval names its recipient, amount and effect, expires, and is bound to the exact action approved, in the spirit of Google AP2's intent and cart mandates. The payments floor stays a kernel floor. (P1)
- **The audit trail a person can read.** Every agent action records who it acted for, the tool, the arguments with secrets redacted, the approval and the result. The person sees their own trail and the Team Admin sees the workspace's. The Magnus admin sees counts only. (P1; most of this exists in the boundary log and receipts, but only in the developer view.)
- **MCP and OAuth.** Keep audience-bound tokens and per-client consent, and never pass a person's token through to another server. The connectors split on 2026-11-14 made these checks pure and tested; add them to the conformance suite run on each release. (P1)

---

## 6. UI/UX

### 6.1 First run and onboarding (P1)
Today: sign-in, certificate trust, Terms, usage-data prompt, then one line of help. Replace the single line with a three-step start that gets the person to **one real result**:
1. Their time zone (detected) and name, confirmed.
2. "Connect one app you use" (mail, calendar or a task tool), with Skip.
3. A suggested first task built from that app ("Summarise what's waiting in my inbox"), run with them watching. Its first ask explains what asks are.

After that, a short "Your computer" card says what the computer is, that it's theirs, and how to take over.

### 6.2 States
- **One connection state.** Today a lost connection shows many "may be stale" lines. Show one banner, "Reconnecting to the server… your work keeps running there", keep drafts, and queue messages typed while offline. (P1)
- **Empty states** that explain and offer one action, such as Files: "Nothing here yet. Files the agent makes, and files you add, appear here. [Add a file]". (P2)
- **Errors** that say what happened, why, and what to do. Every error the server returns already has a message; check that the app shows it and never only a status. (P1)

### 6.3 Accessibility (WCAG 2.2 AA) (P1)
The base is strong. To finish it:
- remove `outline:none` on the composer and library search (`app.css:383,535`);
- give every control a visible `:focus-visible` style;
- make targets at least 24×24 px;
- announce live agent output through an `aria-live` region at a sensible rate;
- keep focus from being hidden under sticky bars (2.4.11);
- make the "Accessible Authentication" rule (3.3.8) hold for two-factor: allow paste, and passkeys count.

Run axe in the desktop node checks.

### 6.4 Language (P2, plan it now)
All strings are hard-coded English, and the spelling mixes "organisation" and "organization". Pick one spelling now (P1). Move strings into one table with ICU plural forms, so a second language is a translation and not a code change.

### 6.5 One UI (P1), following "team chat = the personal chat with many users"
- Merge personal and team **Activity** into one view, with a filter.
- Make **Ctrl/Cmd+K** global: today it works only under `#/team/`. It searches everything the person may see.
- Pick one word: the app says both "Teams" and "Workspaces".
- Settle "Computers": the nav shows cloud and local together, and "this computer" means both. Name them "Your computer" (cloud) and "This device".

### 6.6 Readability of asks (P1)
The ask card shows raw field keys and `JSON.stringify` output (`asks.js:18-31`). Draw each ask kind as a sentence plus a table of what will happen: recipient, amount, the diff of a file, the text of a message. Keep the raw form behind "Details". Show *why* it asked; today that appears only in the developer view.

### 6.7 Size
`desktop/ui/app.js` is 3211 lines and `app.css` 833, and `ci/ratchet.sh size` does not cover `desktop/ui`. Bring the UI under the same 800-line rule, list the two files in `oversize.txt`, and shrink them as screens move into their own modules. (P2)

---

## 7. Teams

### 7.1 Fix (P0/P1)
- **Removal hand-off**, as in P0 #1.
- **Routines are half-built.** There is POST and DELETE only, with no list, edit or UI. Add a Routines list in the workspace panel for Team Admins (what, how often, where it posts, last run, stopped or not) with edit and remove. A stopped routine (2026-11-14 decision) shows why and offers Restart. (P1)
- **Offboarding.** When a member leaves or is removed, stop or explicitly reassign their schedules, routines, running work and connector grants within the workspace. Never transfer them silently. Show the Team Admin what was stopped. (P1)

### 7.2 Add (P1 unless noted)
- **Team Admin audit view**: who joined, left or changed role, what was deleted or pinned, and every agent action in the workspace. Readable by Team Admins, never by the Magnus admin.
- **Workspace export** for Team Admins: messages, files and the knowledge base.
- **Per-member spend inside a workspace**, so a Team Admin sees who uses the shared budget.
- **Search filters**: from a person, in a channel, before/after a date, has a file.
- **Channel browser**: join and leave public channels.
- **Status**: away, and a "back at" message tied to quiet hours. (P2)
- **User groups** (@designers). (P2)
- **Scheduled messages and reminders** in channels: these are routines with a message target, so build them on routines. (P2)
- **Guests**: today a guest has the agent do nothing. Add single-channel guests (sees only that channel, no directory). (P2)
- **Agent disclosure in a channel**: the channel header says whose authority the agent acts on, what it may use, and that it never brings one member's private data into the channel.

### 7.3 Keep as decided
- Two stores: personal words stay in the person's store and team words in the ledger.
- Private reactions in the personal chat; public emoji in teams.
- Team delete is final and personal delete goes to the trash, and the wording never promises otherwise.
- No sharing across scopes.

---

## 8. Autonomy

### 8.1 Control (P1)
- **The person's own stop button.** The spec says the person, an admin or the monitor can "pause or destroy a computer and revoke all its grants in one step". Today only admins have the kill switch, and a person who is paused sees "Paused by the kill switch. Resume it…" with no control (`screen.js:107`). Add "Stop everything" for the person's own work: their runs, schedules and grants. It ends at the next tool boundary and revokes in-flight tokens. Resuming is the person's own choice, unless an admin's switch holds them.
- **Undo for reversible work.** Tiers already mark actions "can be undone". Give each finished task an **Undo what it changed** action covering its reversible writes (files restored from the computer's snapshot, a draft deleted, a calendar entry removed). Each irreversible step is listed as done and not undoable. Stop stays as it is.
- **Interrupt and redirect.** A message sent while a task runs goes to that task ("also include March"). The person should not have to stop it and start again. The run reads it at its next step.

### 8.2 Trust ramp (P1)
Autonomy should grow with a track record, and only by the person's choice:
- When the same kind of ask has been approved unchanged *N* times (say five) with no "I had to undo it", offer a standing-authority card: "Magnus has asked to file receipts in Drive 6 times and you approved each one. Let it do this without asking?" It covers that scope only, can be revoked, and is listed in Settings › Rules. The data is already in Measured outcomes; it is simply not shown to the person.
- Show the person their own record in Settings: tasks done, approved unchanged, corrected, undone. They can then judge for themselves.
- Never widen anything automatically. Floors (payments, irreversible actions) never become standing authority.

### 8.3 Visibility (P1)
- **The ideas feed and goals**, as promised by the spec ("Suggestions land in one feed… Long-running projects live as goals with a plan and visible progress"). Today suggestions are only chat cards. Add a Suggestions feed, which can be dismissed, snoozed or turned into a task, and a Goals view with plan, progress and next step.
- **Your usage**, as in 3.3.
- **Schedules show local time**, as in 3.2.

### 8.4 Unattended behaviour to keep
These are already built well; keep them:
- an unanswered ask leaves a draft after 30 minutes;
- failing schedules and routines stop and say so;
- per-task budgets;
- the reviewer and verifier;
- receipts.

---

## 9. Operations: from dev to production

### 9.1 Release discipline (P0)
Today **every push to `main` installs onto the production server**: the production server follows the test channel, and the release job does not wait for the fast tier. This is the most important change.

1. **Separate the test server from production.** Run the test channel on a test server (Strix Alpha or Beta, or a VM). Production follows a **stable** channel.
2. **Make the release depend on the fast tier** (`needs:` in `release.yml`, or a `workflow_run` on fast success). A red or cancelled fast run never publishes.
3. **Promotion is a step, not a push:**
   - test → **candidate**, after `ci/acceptance.sh` is green on Strix against the exact artifact: autonomy, injection, update, evidence, invariants, kill switch, desktop look, and contracts with the owner's test accounts;
   - candidate → **stable**, after 48 hours on the owner's own use with no regressions;
   - tag each stable release in git.
4. **Fix the 3 invariant failures** and the weak `never()` oracles (an invariant edit for the owner) before the first stable release.
5. **Desktop staged rollout.** The update feed serves stable to a percentage (10% → 50% → 100%) and halts automatically on a crash or error spike, with a one-step rollback. Desktop production is at 0.7.0, about 150 builds behind; cut a stable desktop with the server.
6. **Migrations**: back up before migrating, and keep a migration that can't be reversed behind a release note. This already holds for the event log by design; write it down for the Postgres tables.

### 9.2 Host (P0)
- **Apply the network fixes**:
  - a Wi-Fi watchdog that restarts the interface when RX stalls;
  - SSH allowed from the wired subnet;
  - `never-default` on wired profiles;
  - better, move production onto wired Ethernet.

  These are host changes for the owner.
- **Memory.** `max_live = 8` × 4096 MiB is 32 GiB on a 28 GB host. Lower `max_live` to 5, or the per-VM size, and put an alert at 85% memory. This overcommit is the likely trigger of the Wi-Fi stalls.
- **Disk.** It is at 79%, and backups grow about 10× a day at the start. Alert at 85%, keep backups off the system disk, and set a Docker image prune.
- **Split the roles.** The production host also runs magnus-claude's Postgres, searxng, a preflight Postgres and the GitHub runner holding the release key. Move the release runner and key off the production host (Strix Alpha, or a hardware key). Production runs only production.
- **HA**: not needed for the first release. Write down the recovery time instead (9.3).

### 9.3 Data protection (P0)
- **3-2-1 backups**: today's daily encrypted file, plus an **off-site copy** (object storage with object lock, or the owner's NAS), plus one copy offline or immutable.
- **Key off the server**: the backup key goes to the owner's password manager and a printed sheet. The server holds only what it needs to write backups.
- **Postgres point-in-time recovery**: WAL archiving to the off-site store, so the worst-case loss falls from 24 hours to minutes.
- **Targets**: RPO 15 minutes and RTO 4 hours, written down.
- **A restore drill each month** on Strix from the off-site copy, timed and recorded in `decisions/`. An untested backup is not a backup.

### 9.4 Observability (P0 for alerting, P1 for the rest)
- **External uptime check** on `https://teams.pyrallon.ai/v1/health` from outside the network, alerting the owner by phone (ntfy, Pushover or a hosted uptime service).
- **The host health check probes Magnus itself**: the health endpoint, the sweep heartbeat, backup age, update status and computerd. Today it checks disk, Docker and the firewall only.
- **Metrics**: an OpenTelemetry or Prometheus exporter for request rates and errors, run outcomes, model latency and tokens (GenAI semantic conventions), spend, VM count, memory, disk, and backup and update results.
- **SLOs** to start: API availability 99.5% monthly; ask delivery under 1 minute at p95; scheduled runs start within 2 minutes of due at p99; zero missed or doubled schedule firings. Set an error-budget policy: when the budget is spent, pause feature releases.
- **Error reporting** from the desktop app, opt-in under the existing usage-data consent.
- **A status page** hosted off the production host.

### 9.5 Capacity and dependencies (P1)
- **Model failover** across the configured adapters (4).
- **Choose a search provider** (owner decision).
- **Request rate limits** (5.1).
- **A load test** on Strix: N people with M concurrent runs, schedules firing together, and a team channel with the agent addressed by many people.
- **Failure drills**: model provider down, Postgres restart, VM capacity exhausted, disk full, network loss. Each should end in a stated, user-visible state, never a silent one.

### 9.6 Support (P1)
- **Runbooks** in `docs/runbooks/` covering: server won't start; restore from backup; roll back a release; rotate the vault or backup key; network loss; disk full; a person locked out; kill switch pressed.
- **An incident process**: severity levels, who is told, and a blameless postmortem in `decisions/`.
- **Support sessions a person grants**, as in the spec. A person can open a time-boxed session that lets support see a named part of their data. It ends on its own and is listed in their audit trail. Today there is none (`person_admin.rs:10`).

### 9.7 Legal and privacy (P0 before others use it)
- Fill in `[legal]` in production (operator, contact, governing law) and **host the Terms and Privacy pages** (`pyrallon.ai/terms` and `/privacy` return 404).
- **Name each subprocessor** in the privacy notice: Anthropic, OpenAI and the other model providers, Cloudflare, Stripe, mail. Say what each receives and for how long.
- A **data processing agreement** template for organisations (GDPR Art. 28).
- **Agent transparency**: the agent's messages and actions say they come from an AI (EU AI Act Art. 50, applying from August 2026). Mail and messages sent for a person carry their name, with the agent disclosed according to the person's rules.
- **GDPR Art. 22**: the agent never makes a decision with legal or similarly significant effect on someone without a person's approval. The ask floor already covers this; say so in the privacy notice.
- **Licences**: the project is `UNLICENSED`. Run `cargo-deny` to check dependency licences against the intended distribution.
- **SOC 2** is not needed for the first release. Collect evidence from the start (access reviews, change management, incident log, backup drills) so a Type I is cheap later. (P2)

---

## 10. Release plan

**Stage 0 — stop the bleeding (this week).**
- P0 fixes 1–7.
- The release gate (9.1, steps 1–2): production off the test channel.
- Network fixes and the memory limit (9.2).
- Off-site backups with the key off the server (9.3).
- External uptime alerting (9.4).
- Credential files into the vault.
- `[legal]` and the hosted pages.

**Stage 1 — closed beta (2–4 weeks).** For the owner plus up to 10 invited people, with Tester Terms on.
- Time-zone UI.
- Self-service two-factor and passkeys.
- Self-service export and deletion.
- Connected apps.
- Sessions and devices.
- The person's stop button and usage view.
- Readable asks.
- Onboarding.
- One connection state.
- Routines UI.
- Offboarding.
- Rate limits.
- Model failover.
- The acceptance gate for promotion (9.1 step 3).
- Invariants at 123/123.
- Restore drill #1.
- Runbooks.

Beta exit criteria:
- Two weeks inside the SLOs.
- No P0 or P1 bugs open.
- One restore drill passed.
- One failure drill per dependency passed.
- An ASVS L2 self-assessment with no open high items.

**Stage 2 — production release 1.0.**
- Staged desktop rollout.
- SBOM and provenance.
- `security.txt`.
- Agent audit trail for people and Team Admins.
- Trust ramp.
- Ideas feed and goals.
- Team Admin audit and export.
- Search filters.
- WCAG 2.2 AA pass.
- Status page.
- Error reporting.

The release is tagged and the stable channel is promoted.

**Stage 3 — after 1.0.**
- SCIM.
- Languages.
- Guests.
- User groups.
- Scheduled messages.
- Status/away.
- The server-side theme.
- UI files under 800 lines.
- SOC 2 readiness.
- HA, if usage calls for it.

---

## 11. Decisions the owner needs to make

1. **Take production off the test channel** and gate releases (9.1). This is a production change. Recommended: yes, now.
2. **Host changes**: the network fixes, the memory limit, moving the release runner and key off production, LUKS (9.2, 5.1). These are host changes. Recommended: yes.
3. **Off-site backup target** and where the backup key is kept (9.3). Recommended: object storage with object lock, and the key in the owner's password manager.
4. **Legal details** for `[legal]` and the hosted pages, and the subprocessor list (9.7).
5. **A search provider** (9.5).
6. **Self-service account deletion** replacing the admin-approved request (3.3). This is a change to a promise about data. Recommended: yes, because it is the safer reading of "a Magnus admin reads nothing".
7. **Support sessions a person grants** (9.6). The spec has them; confirm the scope.
8. **The weak-oracle invariant edits** (9.1 step 4).

Everything else in this plan follows from the spec, the decisions and the owner's rules, and can be built and recorded as decisions without further approval.

---

## Sources

- NIST SP 800-63B-4: authenticators, AAL and sessions — https://pages.nist.gov/800-63-4/sp800-63b/
- OWASP ASVS 5.0 — https://github.com/OWASP/ASVS
- OWASP Top 10 for LLM Applications 2025 — https://genai.owasp.org/llm-top-10/
- OWASP Top 10 for Agentic Applications 2026 — https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/
- OWASP Session Management, Authentication, Logging and Secrets Management cheat sheets — https://cheatsheetseries.owasp.org/
- MCP security best practices — https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices
- OAuth for native apps, RFC 8252; OAuth security BCP, RFC 9700; token revocation, RFC 7009; SCIM, RFC 7644; security.txt, RFC 9116
- Tauri v2 security — https://v2.tauri.app/security/
- SLSA — https://slsa.dev/spec/v1.0/levels · Sigstore — https://docs.sigstore.dev/ · CISA SBOM — https://www.cisa.gov/sbom
- Anthropic, Building effective agents — https://www.anthropic.com/engineering/building-effective-agents
- Anthropic, framework for safe and trustworthy agents — https://www.anthropic.com/news/our-framework-for-developing-safe-and-trustworthy-agents
- Anthropic computer use tool — https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/computer-use-tool
- OpenAI, A practical guide to building agents — https://cdn.openai.com/business-guides-and-resources/a-practical-guide-to-building-agents.pdf
- Knight Institute, Levels of autonomy for AI agents — https://knightcolumbia.org/content/levels-of-autonomy-for-ai-agents-1
- Microsoft HAX guidelines — https://www.microsoft.com/en-us/haxtoolkit/ai-guidelines/ · Google PAIR guidebook — https://pair.withgoogle.com/guidebook/
- Google AP2 (agent payments) — https://cloud.google.com/blog/products/ai-machine-learning/announcing-agents-to-payments-ap2-protocol
- NIST AI 600-1 — https://nvlpubs.nist.gov/nistpubs/ai/NIST.AI.600-1.pdf
- WCAG 2.2 — https://www.w3.org/TR/WCAG22/ · ARIA APG — https://www.w3.org/WAI/ARIA/apg/
- Google SRE: PRR, SLOs, canarying, incidents — https://sre.google/
- OpenTelemetry GenAI conventions — https://opentelemetry.io/docs/specs/semconv/gen-ai/
- CISA backup and ransomware guidance — https://www.cisa.gov/stopransomware/ransomware-guide
- GDPR Arts. 17, 20, 22, 28 — https://gdpr-info.eu/ · EU AI Act Art. 50 — https://artificialintelligenceact.eu/article/50/
- Slack roles, retention, DND — https://slack.com/help

The figures from NIST were checked against the primary text. Other sources were read through summaries; check the primary document before quoting exact wording.
