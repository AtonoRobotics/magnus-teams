# The admin console against its spec

The owner's "Magnus Admin Console Spec" (rev 12,
https://claude.ai/artifact/3Ej2KCssZbFYfR4p1LEwGP) is built as written,
amended only by the owner's rulings (`docs/proposals/spec-decisions.md`;
ruling 1: no passkeys, admins sign in and step up with their password and
code). The console is done when every line below is checked, each with the
test that proves it. Nothing is reported done before that (owner,
2026-11-02: "do not build min slices and give up before the job is done").

Each line names the pull request that built it, or the one building it.

## Access

- [x] Its own process, `magnus-admin`, bound only to configured internal addresses: never every address, never loopback (#68; `admin_reach.rs`)
- [x] Only the organisation's LAN and VPN subnets; loopback refused, so a misrouted tunnel is refused (#68; `admin_reach.rs`)
- [x] The configured hostname only, else 404; every name it is reached by (#68, #79; `admin_reach.rs`, `admin_on_appliance.rs`)
- [x] The tunnel's public names shown under Operations › Network, the console never among them (`admin-operations`); the ingress itself is the tunnel's config
- [x] TLS 1.3 only (#68; `admin_reach.rs`)
- [x] A certificate browsers already trust: ACME DNS-01 under the tailnet's domain, issued through tailscaled (Let's Encrypt) for the console's Tailscale name and renewed by the console; or the organisation's own CA (`admin-certificate`; `admin_certificate.rs`, `admin_on_appliance.rs`)
- [x] The kill switch from the appliance's own console: `magnus kill` / `magnus release` (before #68)

## Authentication and session

- [x] The `magnus-admin` confidential client, Authorization Code with PKCE, the one redirect URI per name, `private_key_jwt` (#69, #79)
- [x] Back-channel calls to the provider's internal address, issuer unchanged (#69)
- [ ] The role `admin`, checked on every request from the session's validated token. Built: checked at sign-in and its standing checked with the kernel at each refresh, at most every 4 minutes (#69, #508; `admin_sign_in.rs`); not on every request
- [x] Admins sign in and step up with their password and code (ruling 1, no passkeys) (#69)
- [x] Server-side session, cookie `__Host-magnus_admin`, HttpOnly, Secure, SameSite=Strict (#69)
- [x] 30 minutes idle, 12 hours absolute (#69); adjustable under Security › Sign-in (`admin-security`; `admin_security.rs`)
- [x] A CSRF token on every change (#69)
- [x] Sign-out ends the console's session and the provider's (#69)
- [x] An admin turned off at the provider loses the console within 5 minutes (#69; `admin_sign_in.rs`)
- [x] Step-up within 5 minutes for each action the spec names (#69, and each section's own)
- [x] Engaging the kill switch never needs step-up (#71)
- [x] A strict Content-Security-Policy; nothing from anywhere but the appliance (#68)

## Authority and audit

- [ ] Only `magnus-admin` holds `magnus_root` (BYPASSRLS). Built: `magnus migrate` creates the role (#70), but the console never connects as it, and its password is in the config file `magnus serve` also loads
- [x] Every read or write of a person's private data through the console is audited: admin, person, resource, action, time (`admin-users-detail`; `admin_people.rs`)
- [x] Deleting a person's data leaves it unrecoverable: removed from the store and every backup (no per-person key exists: DECISIONS.md) (`admin-users-detail`; `admin_people.rs`)
- [x] Credentials are write-only everywhere (before #68; the console's routes, `admin_connectors.rs`)
- [x] The last admin cannot be removed, turned off or demoted (#72; `admin_users.rs`)
- [x] Every console action in the append-only log with who, what, when, source and before/after; `magnus_root` cannot change it (#70; `admin_audit.rs`)

## Web UI

### Shell

- [x] Top bar: organisation, status dot, kill switch, the admin's name (#68, #71); the app's own fonts and colours in every look (`admin-shell`)
- [x] The admin's menu: theme (light, dark, match computer; solid or glass) and sign out (`admin-shell`; `admin_shell.rs`)
- [x] Five sections in a sidebar, a bottom tab bar at phone width, the kill switch staying in the top bar (#68)
- [x] "Stop all agents", held for one second; the red banner on every screen with who, when and Release (#71)
- [x] Lists open a detail panel on the right; full screen at phone width (`admin-shell`; `admin_shell.rs`)
- [x] Reversible changes show a toast with Undo (`admin-shell`; `admin_shell.rs`)
- [x] Destructive changes open a confirm sheet; the riskiest ask for the target's name typed (`admin-shell`; `admin_shell.rs`; typed, under Users › Delete)
- [x] Step-up opens the console's own sign-in in a sheet and returns to the action (`admin-shell`; `admin_shell.rs`)
- [x] Live data over server-sent events, `/api/v1/events`: status, the kill switch, tasks, microVMs and knowledge changes (`admin-shell`, `admin-operations`, `admin-knowledge`)
- [x] Designed empty, loading and error states on every screen built so far (`admin-shell`), and on each section as it is built

### Overview

- [x] Status strip: health, kill switch, version (#71)
- [x] Status strip: the tunnel connected or not (`admin-operations`)
- [x] Cards: running tasks, people signed in, knowledge changes in 24 hours (#71)
- [x] The knowledge card opens Activity (`admin-shell`)
- [x] Alerts: a service down, disk over 85% (#71)
- [x] Alerts: a certificate expiring within 14 days (`admin-shell`; `admin_shell.rs`, `admin/cert.rs`)
- [x] Alerts: the tunnel down; a model provider unreachable (`admin-operations`)

### Knowledge

- [x] Ontology: searchable types; definition, attributes, relations with cardinality, a graph of related types, entries referencing it, history (`admin-knowledge`; `admin_knowledge.rs`)
- [x] Ontology: create, edit, add or remove a relation, merge, deprecate, delete (refused while in use); every change versioned, diff per version, rollback (`admin-knowledge`; `admin_knowledge.rs`)
- [x] Context: the map, who does what, terminology and standing facts, sources; who last changed each and when; edit in place; "As agents see it" (`admin-knowledge`; `admin_knowledge.rs`)
- [x] Memory: searchable table (summary, entities, source, created, last used) with filters; detail with provenance, entities, versions; edit, delete, rollback; bulk delete and rollback with step-up (`admin-knowledge`; `admin_knowledge.rs`)
- [x] Activity: every agent change, newest first, with before/after and source; revert one; revert a selection with step-up; filters; live (`admin-knowledge`; `admin_knowledge.rs`)

### Users

- [x] List: name, email, role, status, last sign-in, sessions; Invite, Resend, Cancel (#72)
- [x] Overview tab: role (step-up), turn off or on, end all sessions; turning off pauses their schedules (#72)
- [x] Sessions tab, with End (#72)
- [x] Schedules and triggers tab, with Suspend and Resume; Magnus has no triggers but schedules (`admin-users-detail`; `admin_people.rs`)
- [x] Data tab: their tasks, chats, files and memory, and one of each, each access audited and told to them, with the banner saying so; none while the policy keeps data private (ruling 14) (`admin-users-detail`; `admin_people.rs`)
- [x] Delete tab: the account and everything of theirs, from the store and the backups, with step-up and their email typed (no per-person key exists: DECISIONS.md) (`admin-users-detail`; `admin_people.rs`)

### Security

- [x] Sign-in: member methods (password, and whether a code is required), admin method shown fixed, session lifetimes for members and the console, lockout thresholds; applied to the kernel's sign-in rules, which it enforces on the local path, with step-up (`admin-security`; `admin_security.rs`)
- [x] Agent policy: each Cedar policy set in an editor with highlighting and inline validation errors against Magnus's schema (`admin-security`; `admin_security.rs`, `policy_admin.rs`)
- [x] Agent policy: a test panel (principal, action, resource, context → the decision and the policies that decided it) (`admin-security`; `admin_security.rs`)
- [x] Agent policy: a diff against the live version; Apply with step-up; history and rollback (`admin-security`; `admin_security.rs`)
- [x] Connectors and credentials: the organisation's connectors with status, scopes, last used; add, turn off or on, who may use it, set, rotate, revoke, remove; credentials write-only, shown "set" and when (`admin-connectors`; `admin_connectors.rs`)
- [x] Audit: the whole log, filtered by actor, action, target and time, with CSV (#70)

### Operations

- [x] Kill switch: state, who and when, tasks halted, schedules held (#71)
- [x] Kill switch: microVMs frozen; Release with step-up from here (`admin-operations`; `admin_operations.rs`)
- [x] Modules: name, version, status, what each provides; enable, disable (`admin-operations`; `admin_operations.rs`)
- [ ] Install from a package and remove. Built: both answer 409 and say Magnus's modules come with its release; the spec's open question 22 (what "module" names) is not ruled, so this waits for the owner
- [x] Model providers: enabled, endpoint, models, a connection test, last error; never a credential (`admin-operations`; `admin_operations.rs`)
- [x] Health: kernel, event log, admin console, tunnel; CPU, memory, disk; recent errors (#71)
- [x] Health: cloudflared, running microVMs, live (`admin-operations`; `admin_operations.rs`)
- [x] Updates: version, available update with notes, install and rollback with step-up; the desktop app's channels and published versions, and everyone's channel (`admin-operations`; `admin_operations.rs`)
- [x] Network: tunnel status and public hostnames, the admin hostname, the allowlist (a change that would shut out the admin's own address refused), certificate expiry (`admin-operations`; `admin_operations.rs`); Renew, with why the trusted certificate could not be issued (`admin-certificate`; `admin_certificate.rs`)

## Server and API

- [x] One Rust binary serving the UI and a JSON API under `/api/v1` (#68)
- [x] The kernel over a local Unix socket (#70)
- [ ] Postgres as `magnus_root`, insert and select only on the log. Built: the role and its grants (#70); the console reads and writes through the kernel's control socket and never connects as it
- [x] User management through the kernel's local-store endpoints over the control socket (#508); the provider's admin API (`magnus-admin-svc`) is deleted with the realm's console client
- [x] The kill switch the kernel's own, persisted, refusing new runs while on (#71, before)
- [x] Every endpoint in the spec's table (`admin-shell` to `admin-certificate`); `POST /modules` and `DELETE /modules/{id}` answer 409 until the owner rules on the spec's open question 22 (see Modules)
- [x] Every change audited before it answers; a step-up endpoint without a fresh check returns 401 `step_up_required` (#70, #69)

## Invariant tests

Reach
- [x] Nothing on the public side serves a console page or admin route (`admin_reach.rs`)
- [x] Loopback refused (`admin_reach.rs`)
- [x] Outside the allowlist refused; an address inside it accepted (`admin_reach.rs`)
- [ ] A connection from the VPN subnet accepted: no test connects from a VPN address
- [x] The wrong Host gets 404 (`admin_reach.rs`)

Sign-in and authority
- [x] A member's session or token is refused on every admin endpoint (`admin_sign_in.rs`)
- [x] An admin cannot sign in without their password (ruling 1) (`admin_sign_in.rs`)
- [x] Every step-up endpoint returns `step_up_required` without a fresh check and succeeds with one: kill switch release (`admin_sign_in.rs`), role change and admin invite (`admin_users.rs`), delete (`admin_people.rs`), sign-in rules, policy apply and rollback (`admin_security.rs`), memory bulk and activity batch (`admin_knowledge.rs`), update install and rollback, allowlist (`admin_operations.rs`)
- [x] An admin turned off loses the console within 5 minutes (`admin_sign_in.rs`)
- [x] The last admin cannot be removed, turned off or demoted (`admin_users.rs`)

Data and audit
- [x] As the user API's role, a cross-person read returns nothing (before #68)
- [x] An admin's read of a person's data succeeds and is in the audit log with admin, person, resource and time (`admin_people.rs`)
- [x] No admin endpoint ever returns a credential value (`admin-connectors`; `admin_connectors.rs`, every reading route and the stream; re-run as each section adds routes)
- [x] `magnus_root` cannot change an audit entry (`admin_audit.rs`)

Knowledge
- [x] An admin edits an ontology type and a memory entry, sees provenance on both, and rolls each back (`admin_knowledge.rs`)
- [x] An agent's change to org memory appears in Activity, and reverting it restores the prior state (`admin_knowledge.rs`)

Kill switch
- [ ] Hold-to-stop at phone width halts a running task and a pending schedule within 5 seconds. Built: the kill switch halts both (#71); no test holds the button at phone width or times the stop
- [x] Engaged across a restart, and no new run starts (before #68)
- [x] `magnus kill` engages it with the network down (before #68)
- [x] Release without step-up fails (#71)

UI
- [ ] Every screen in light, dark and glass, and at phone width with no horizontal scroll, the sections in reach. Visited through `admin-sign-in.mjs`: Users, a person, Security › Sign-in, Agent policy and a connector, Operations › Network; not yet Overview, Knowledge, Audit, Modules, Model providers, Health or Updates

## Removals

- [x] No `admin.pyrallon.ai` ingress, no Cloudflare Access application, no admin route on the user API, no admin listener on every address or loopback (checked in #68; Network shows the tunnel's names)
