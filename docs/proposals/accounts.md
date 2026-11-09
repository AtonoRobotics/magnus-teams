# Accounts: Keycloak through one identity interface

Status: building. Step 1 (sign-in) is built. This plan follows the owner's
decisions of 2026-10-30, newest last:

1. Use the owner's Keycloak as the identity provider now, through the
   identity interface. Map Keycloak groups to Magnus roles. A disabled user
   loses access and has their schedules suspended within minutes (K54).
   Keep local accounts as a fallback.
2. "There should never be a fake." Every test runs against a real Keycloak
   and a real browser.
3. "The point of this is that a non technical user can just connect the app.
   If they need to worry about certificate installs and TPM and relays it's
   easier to give them a vpn." Sign-in "needs to be username and password
   just like logging into muse or grok bot."
4. Option 1: the app comes set up for its appliance, downloaded from a link
   the owner sends or from the appliance, and opens straight to username and
   password. Passkeys stay as an option, in the browser.
5. "Open browser the first time or if user is not found."
6. "The back up password is only for me. It should not be allowed for anyone
   else."
7. "Make it available only from the appliances own machine."

## Sign-in (built)

**What the person does.** They type their username and password in the app.
If they use an authenticator app, a second screen asks for the 6-digit code.
That's all, except in the cases under "The browser" below.

**What happens.**
- The app sends the username and password to the appliance over its pinned
  HTTPS connection.
- The appliance checks them with Keycloak's password grant ("Direct access
  grants"). The password is never stored or logged.
- Keycloak gives the same answer to a wrong password and to a missing code.
  So the appliance asks Keycloak, as a service account that may only view
  users (`view-users`), whether this person uses a code. If they do, the app
  asks for it. It asks whether or not the password was right, so the
  question tells nobody the password was right.
- Wrong username, password or code: the person is told so. Too many wrong
  tries lock the name for `lockout_minutes`, as local passwords do.

**The browser.** The app opens Keycloak's own page in the person's browser,
with their username filled in:
- the first time they sign in to this appliance, where Keycloak can walk a
  new account through what it needs (a new password, setting up two-factor,
  updating account details);
- when Keycloak says the account isn't fully set up (it never runs those
  steps for a password typed elsewhere);
- when Keycloak knows no such username;
- when the person picks **Use a passkey instead**. Passkeys need the
  browser: the Linux app's webview has no WebAuthn (measured: WebKitGTK 2.52
  has no `navigator.credentials`).

The browser sign-in is the authorization code with PKCE (S256), coming back
to a loopback address on the person's computer (RFC 8252). The appliance
makes and keeps the verifier, state and nonce, and checks the ID token's
issuer (RFC 9207 `iss` too), audience, expiry and nonce. The appliance's own
page in a browser uses the device grant for the same cases.

**The owner's backup.** A password set on the appliance (`magnus password`)
signs in only the owner, only on the appliance's own machine (a loopback
connection with no proxy headers), when Keycloak is down or refuses. From
anywhere else, Keycloak's answer stands, so the backup cannot skip
Keycloak's two-factor. It is tried
before Keycloak's "use the browser" or "enter your code" answers, so the
owner never needs the browser. `magnus password` refuses anyone else, and a
password someone had before Keycloak was set up stops counting, so turning
a person off in Keycloak shuts them out. With no `[sign_in.oidc]`, these
passwords are everyone's way in.

**Trade-offs the owner accepted.** The password passes through the
appliance (encrypted, never kept). Passkeys don't work in the app's form.
Keycloak needs Direct access grants turned on for the client.

## The app comes set up for its appliance (next)

Today the person types the appliance's address and compares a certificate
fingerprint. Option 1 removes both.

- The owner sends a link. It opens a page that offers the app's download
  for the person's system, and then an **Open Magnus** button.
- The button is a `magnus://connect` link carrying the appliance's address
  and its certificate fingerprint. The installed app registers the
  `magnus://` scheme, pins the fingerprint from the link, and opens straight
  to username and password.
- The link the owner sends is what the person trusts, as they trust the
  owner's message. The person never sees a fingerprint.
- The appliance also serves the same page to anyone on the office network.
  The fingerprint on that page is only as good as that network, so the owner
  sends the link when the network isn't trusted.

## Device certificates (after that)

Simplified to what a non-technical person never sees:
- At the first sign-in on a computer, the app makes a key in the operating
  system's keychain (Windows: a non-exportable CNG key; Linux: the Secret
  Service). No TPM setup, no certificate install.
- The appliance issues the app a client certificate for that key, naming
  the person and the computer. The session is bound to it.
- Every request over the network must come with a valid certificate whose
  person matches the session. A token copied off the computer is useless
  elsewhere.
- **Your devices** in Settings lists each computer (name, last used) with
  **Remove**. The owner can remove anyone's. Deactivation removes all of a
  person's.
- A key in the keychain can be copied by someone who controls the computer.
  The owner accepted this.

## Groups to roles

`[sign_in.oidc.group_roles]` maps a Keycloak group to Magnus roles, for
example `"approvers-financial" = ["approver:financial"]`. Groups are read at
each sign-in and each refresh, so a group change alters approval rights
within one access-token lifetime.

## Short sessions and the status check (K54)

- A Magnus session lasts as long as the person's Keycloak session. The
  appliance refreshes it when Keycloak's access token expires (`expires_in`;
  Keycloak's default is 5 minutes). A refused refresh ends the session.
- Every `status_minutes` (default 5, matching that lifetime), the status
  client asks Keycloak about each person who signed in through it.
  Schedules act for people who may not be signed in, which is why this is
  needed.
- **Disabled or deleted:** every session and device of theirs is refused.
  Their schedules and running tasks are suspended. People with the owner
  role get one inbox notice per schedule, with **Reassign** or **End**.
- **Keycloak unreachable:** retried; nobody is deactivated; Home says the
  check is failing.

## The relay (waits for a public host)

The relay is a forwarding point on the public internet. It lets the app
reach the appliance from outside the office without changing the office
router. It needs a public name with HTTPS on port 443, a certificate, and a
small program that passes bytes between the app and the appliance.

- **A. A Cloudflare Tunnel hostname.** This machine already runs one for
  production. It needs no new server, but Cloudflare decrypts the traffic.
- **B. Our own relay on a small cloud server.** It routes by name without
  decrypting, so only the app and the appliance see the content.
  Recommended for the product.

## Tests

Every test uses a real Keycloak (26.8.1 by digest, `dev/keycloak.sh`), with
a fresh realm per test set up as below, and a real Chromium on Keycloak's
own pages.
- Scratch: password and code sign-in; the code asked for whether or not the
  password is right; first sign-in, an account needing a new password, and
  an unknown username go to the browser; code + PKCE refuses strangers,
  forged states, reused codes, other clients and declines; groups and roles
  come from Keycloak.
- Installed app (Linux): the first sign-in goes through the browser; a later
  sign-in uses the app alone, with the browser made unable to sign anyone
  in.
- Windows: the owner's test.

## Build order

1. Sign-in: username and password in the app, the code, the browser for
   first sign-in, setup, unknown names and passkeys. **Built.**
2. The app set up for its appliance: the `magnus://connect` link and the
   download page.
3. Device certificates and Your devices.
4. Groups to roles.
5. Short sessions, the status check and deactivation.

## Keycloak changes for strix-alpha (made 2026-10-30, except 4)

1. **Client `magnus-claude`:**
   - turn on **Direct access grants** (the password typed in the app);
   - turn on Standard flow, with redirect URI `http://127.0.0.1/*` (the
     browser cases; checked: Keycloak rejects a loopback redirect for this
     client today);
   - require PKCE method S256.
2. **A new confidential client, `magnus-claude-status`:** Service accounts
   only; its service account gets just `realm-management` → `view-users`.
   Its secret goes into the appliance's vault, and `[sign_in.oidc]` names it:
   `status_client = "magnus-claude-status"`,
   `status_credential = "<vault secret name>"`.
3. **Brute-force detection** on in the realm (Realm settings → Security
   defenses). Keycloak leaves it off. The appliance has its own lockout too.
4. **A "Group Membership" mapper** on the client's ID token, for step 4.
