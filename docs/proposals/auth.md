# Authentication and isolation

Status: plan. Source: the owner's design "Magnus Authentication & Isolation
Design" (2026-10-31, https://claude.ai/artifact/NCBiLHkWQ3QGpPUc8W1PcP).
This plan compares the design with what Magnus does today, then orders the
changes into steps. Each step is its own pull request, tested on the real
Keycloak. The design replaces several earlier decisions, listed under
"Replaced" so none of them stays half in force.

Amended by the owner's rulings of 2026-11-01 (`spec-decisions.md`): no
passkeys, and people go on typing their username and password in the app,
so the browser-only sign-in below is not built and DEC 274 and 276 stand;
the admin's step-up is their password (and code). A person's data is the
organisation's by default, with a policy that can make it private.

## Today against the design

| Design | Today | |
| --- | --- | --- |
| Public reach through a Cloudflare Tunnel, valid TLS, no open ports | Self-signed certificate on port 7700, pinned by the app on first use; reached on the LAN or Tailscale. Keycloak is already public at identity.pyrallon.ai | change |
| Browser sign-in only (code + PKCE, loopback), the app never collects passwords | Username and password typed in the app (password grant), the 6-digit code screen, a status client to learn who uses a code; the browser only for the first sign-in, setup and passkeys | change |
| Keycloak tokens: 5-minute access, rotating refresh, kept in the OS keychain | Magnus issues its own 30-day session token and throws Keycloak's away; a file where there is no keychain | change |
| Disabling a person ends access within one token lifetime | A disabled person keeps a live Magnus session until it expires (the status check is planned, not built) | change |
| One JWT check per request (JWKS, algorithm allowlist, `iss`, `aud`, `exp`); identity is `sub` | Opaque token looked up per request; the ID token's signature is not checked; identity is the username | change |
| Two realm roles, `admin` and `member` | `owner`, `developer`, `tester`, `approver:<risk>`, `policy:*`, `knowledge:endorse` | change |
| Postgres row-level security keyed on `sub`, `FORCE ROW LEVEL SECURITY`, non-owner role | Row security on `knowledge` only; per-person visibility of tasks, inbox, library and chat is filtered in application code | change |
| One microVM per user, never reused across users | One microVM per run, its disk deleted when the run ends; nothing is reused across users | holds |
| Each user's provider credentials under their own key (envelope encryption) | One appliance key for the whole vault; model and connector sign-ins are one per appliance | change |
| Cedar: the agent is principal, the user is context; `resource.owner == principal` | The person is principal; resources have no owner | change |
| Curation: org knowledge from org-scoped sources; private content only when the user shares it | Already so: an entry from private content is readable only by its person, and endorsing never widens who can read it | holds |
| Not built: own sessions, own passwords, device certificates, token exchange, Keycloak authorization | Own sessions and backup passwords exist; device certificates are planned (`accounts.md`) | remove |

**The recorded sign-in failures** were not an issuer mismatch: the issuer
has always been identity.pyrallon.ai. They were the password grant with a
6-digit code, the status client reaching the public host for an admin API
served only on the admin host, PKCE becoming required for the device grant,
the loopback redirect, and HTTP/2 on the self-signed port. Browser-only
sign-in and the tunnel remove every one of those paths.

## Steps

1. **Public names.** The existing Cloudflare tunnel on the machine also
   carries `magnus.pyrallon.ai` → the appliance (HTTPS and WebSockets).
   Keycloak keeps its public name, identity.pyrallon.ai (the design's
   auth.pyrallon.ai): renaming it would change the issuer and gain nothing.
   Its ingress serves only `/realms/magnus/` and `/resources/` and answers
   404 to everything else, so the admin console is LAN-only, with Cloudflare
   rate limiting on the name. `KC_HOSTNAME` is that public name, with proxy
   headers on. The app connects to `https://magnus.pyrallon.ai` with
   ordinary certificate checks, no pinning. Pinning stays only for an
   appliance with no public name.
2. **Sign-in and tokens.**
   - The app signs in in the system browser (code + PKCE, loopback) against
     the public client, and exchanges the code itself. Passkeys are the
     first way in, password and code the fallback, brute-force detection on.
   - The app keeps Keycloak's refresh token in the OS keychain (Windows
     Credential Manager, Secret Service), rotating on each use, and sends
     5-minute access tokens. Without a keychain it signs in at each start.
   - The appliance checks every request's access token: signature against
     the realm's JWKS (cached, refetched on an unknown key), algorithm from
     an allowlist, `iss`, `aud`, `exp`. A person is their `sub`.
   - Removed: Magnus's own sessions (`accounts.rs`), the backup password and
     the Argon2id store, the password typed in the app and its code screen,
     the status client, the planned device certificates. Scripts and tests
     use Keycloak service-account clients (client credentials) in place of
     the API tokens in `magnus.toml`.
   - Tests: a person disabled in Keycloak is refused within one access-token
     lifetime; a token from another issuer, audience or algorithm is
     refused; an expired one is refused; nothing on the appliance stores a
     password or a session.
3. **Roles.** Keycloak carries `admin` and `member`. The finer grants
   (approver for a risk class, tester, developer, policy layers,
   endorsement) are given inside Magnus by an admin, as they are now set in
   Settings, and stay in the log. `owner` becomes `admin`.
4. **Cedar.** The agent is the principal and the person it works for is in
   the context. Every per-person resource (task, schedule, library item,
   memory, chat thread, inbox item) carries its owner, and the org policy
   allows access when the owner is the context's person. Org knowledge:
   read for every member, write for the curator.
5. **Row-level security.** Per-person data moves into tables with an owner
   column, row security forced, read by a non-owner role with the person's
   `sub` set per transaction (`SET LOCAL`), as `knowledge` already is. The
   API reads a person's things from those tables, not from the kernel's
   in-memory state, so a wrong filter in code cannot show one person
   another's. Org-shared data lives in its own schema, written only by the
   curator. This is the largest step: it changes how every per-person read
   is served.
6. **Per-person secrets.** Each person's credentials (their model accounts,
   their app sign-ins) are encrypted under a key of their own, itself
   encrypted under the appliance key (envelope encryption). Appliance-wide
   keys stay for appliance-wide secrets.

Compute needs no change: a microVM per run is stricter than one per person.

## Replaced

These earlier decisions end with the steps above (DECISIONS.md):

- "People sign in by typing their username and password in the app; the
  appliance checks them with Keycloak's password grant" (owner, 2026-10-30:
  "username and password just like logging into muse or grok bot"), and the
  status client and browser fallbacks that served it. Step 2.
- "Passwords stay open when an identity provider is set, as the backup way
  in", and the owner's backup password on the appliance's own machine. With
  them gone, there is no way in while Keycloak is down. Step 2.
- "No provider access or refresh token is kept", "the ID token is not
  signature-checked", session-token hashes in `state/accounts/`, the token
  file where there is no keyring. Step 2.
- Apps pin the appliance's self-signed certificate. Step 1, for appliances
  with a public name.
- Device certificates (planned in `accounts.md`). Step 2.
- Model and connector sign-ins one per appliance. Step 6: each person signs
  in to their own.

## Open

- The infrastructure for step 1 is on the owner's Cloudflare account: the
  tunnel's ingress rules, DNS for magnus.pyrallon.ai, rate limiting.
- Per-person model accounts (step 6) mean each person signs in to their own
  Codex or Grok; until they do, they have no model.
