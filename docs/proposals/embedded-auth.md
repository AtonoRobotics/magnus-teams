# Embedded auth

Status: plan (#428). Source: the owner's decision, 2026-11-07 — Keycloak
goes; Magnus signs people in itself. This plan replaces the Keycloak
direction (#290, #292, #356, closed as superseded) and the provider half
of `docs/proposals/auth.md` (its token lifetimes, per-request check and
keychain stand; its Keycloak steps go).

## Why

One login, one admin panel, invite signup. The realm served only Magnus
(the appliance API, the desktop app, the admin console), and everything
around it — realm provisioning, realm SMTP, tunnel exposure of `/realms/`
and `/resources/`, subject bindings that break when an account is remade —
existed to keep a second user database alive. The live realm on strix-alpha
is hand-built; it keeps serving sign-in until the cutover, then it goes.

## Non-goals

- Open self-registration: every future is a private appliance, so every
  account starts as an invitation. There is no signup page.
- Federation now: no "sign in with Google", no SAML, no SCIM. The
  `Identity` trait stays the seam; enterprise federation lands on it later.
- Passkeys: the owner's ruling (2026-11-01) stands. Passwords and codes.

## Design

### People

People live in the appliance's Postgres: id, email, name, role
(admin/member), status (invited/active/disabled), an argon2id password
hash, a TOTP secret where enrolled, an invite token with its expiry, and
the audit already recorded (`user_invited` and the rest). Identity is the
local id: there is no provider `sub`, so there is nothing to rebind, and
the subject-binding check goes with Keycloak.

### Sign-in

The person types their username and password in the app, as today (owner's
ruling): the appliance checks the hash, asks for the 6-digit code where
enrolled (the same `NeedsCode` screen), and issues its own session. The
browser code flow and the device-code pairing stay, served by Magnus
itself: the appliance is the issuer its pages name. A wrong password, an
unknown name and a wrong code answer alike, and slowly: attempts are
limited per account and per address.

### Sessions

Magnus already issues its own tokens and throws the provider's away; that
stays, with two repairs: turning a person off ends their access within
minutes (the session is checked against the local store, not only its
expiry), and refresh rotates. Proposed lifetimes: 15-minute access,
30-day refresh, kept as today (the OS keychain, a file where there is
none). Exact numbers are #429's to tune.

### The console

The console stops being an OIDC client: no realm, no `private_key_jwt`
assertion, no JWKS. An admin signs in with username, password and code
against the local store and holds the same server-side session and CSRF
token; only role `admin` enters; step-up stays the password (and code).
The console's key goes once nothing proves with it.

### Users

The Users screen reads and writes the local store through the same routes:
list, invite, resend, cancel, role, disable, end sessions. An invitation
makes the account and a single-use link (a 128-bit token, 7-day expiry):
emailed when the appliance can send mail, else shown in the console for
the admin to pass on (the #352 pattern). Accepting sets the password and
activates. The one-time password stays for people without email.

### Administering from the console (#433)

The console is structurally complete (71 routes, five sections, live on
strix-alpha) yet cannot administer end to end: invites need mail the
realm never had, and users, the appliance and context are not manageable
from the panel in practice. #429 makes each area real, not merely
present, and re-verifies every `docs/admin-console.md` claim live as its
slice lands; a claim that proves false is fixed, not re-documented.

Users: the whole lifecycle from the panel alone — invite, accept, role,
disable and enable, end sessions, delete — against the local store, with
the invite link shown in the console (no email dependency) and the
one-time password staying for people without email. Acceptance: the owner
adds a tester start to finish using only the panel.

Appliance: updates install and rollback exercised live, health truthful
on every service it names, the network allowlist and certificate renewal
usable, the kill switch as today. Acceptance: an update installed and
rolled back from the panel on the build-box appliance.

Context: the bundle sections editable with their content visible, "as
agents see it" matching the assembled context, memory and activity live
and revertible. Acceptance: the owner edits a standing fact and sees it
in the assembled context, and reverts one agent change.

The console's own sign-in moves to the local store with this (above);
the per-request role check and the `magnus_root` question are re-decided
against embedded sessions as those slices land. Module install and
remove still wait on the owner's open question 22.

### Cutover (#429)

Flag day with re-enrollment, not migration: hashes cannot move, and the
base is the owner plus testers. #429 builds embedded auth, switches the
appliance's sign-in to it, re-enrolls everyone through fresh invites, then
deletes the Keycloak surface below. Until the switch, the live realm and
every Keycloak test stay green and untouched.

### Removal list (in #429, after the switch)

- `crates/magnus/src/oidc.rs`: the password, device and browser grants
  against the provider (the `Identity` trait stays for federation).
- The subject-binding check in `crates/magnus/src/api.rs`.
- `admin/auth.rs`: the provider flow, the console key, the JWKS check;
  `admin/directory.rs`: the Keycloak admin calls.
- `config/keycloak-client.py`, `dev/keycloak.sh`, the scratch Keycloak
  harness, CI's Keycloak services.
- `[sign_in.oidc]`; desktop and installed-app sign-in tests rewritten
  onto embedded auth.
- Docs: the provider half of `auth.md`, the Keycloak steps in
  `accounts.md`, the tunnel's identity route.
- The box: the Keycloak container and database,
  `~/.magnus-appliance/identity/`, `identity.pyrallon.ai`.

### Test plan (#429)

Unit: hash and verify vectors, TOTP vectors, invite expiry and single
use, attempt limits. Scratch on a real appliance: invite, accept, sign
in, disable, access ends within minutes; console login and step-up;
invite without email. Installed-app and desktop sign-in through embedded
auth. No fake provider, ever (owner, 2026-10-30).

## Open

- Exact session lifetimes (proposed above; #429 tunes).
- Whether the device-code pairing survives or the desktop moves to the
  browser code flow (#429 decides with the desktop in hand).
