# pyrallon.ai and the tunnel to StrixAlpha

The tunnel is already up. Nothing in this change was created or edited in
Cloudflare: the steps in section 4 are for the owner to apply. Account
`3a1be1145e2314f8612a451e325e0218`, zone `pyrallon.ai` (active; the account
also has `alphavectorrobotics.com`). The appliance is the Linux host
StrixAlpha, on a private network.

Already in place, and left as it is: tunnel `strix-alpha` healthy; ingress
`magnus.pyrallon.ai` to `https://127.0.0.1:7700` with no TLS verify and
HTTP/1.1 to the origin; a proxied CNAME on `pyrallon.ai`. A Cloudflare
Access application with an email one-time PIN currently covers the whole
hostname. That application comes off when the device-certificate rule
below is in place. It is not the way people reach Magnus.

Sources: this repo (`docs/proposals/auth.md`, `docs/connectors.md`,
`docs/admin-console.md`, `docs/proposals/spec-decisions.md`, DECISIONS.md),
`AtonoRobotics/magnus-product` (`deployment/connector-registration.md`,
`workers/connections/oauth_worker.py`), and
`AtonoRobotics/enterprise-agent-spec` (R22: the vendor hosts licensing and
image updates only; customer content stays on the customer's machines).

## 1. What lives on pyrallon.ai

Three different things, on three different names. They are not one service.

| Name | What it is | Where it runs |
| --- | --- | --- |
| `https://pyrallon.ai/` | The public site: homepage, privacy, terms, support contact, and later the Google verification videos. The product registration packet (2026-10-24) says `/privacy` and `/terms` still 404 and the terms are unwritten. Do not publish guessed policy text. | Cloudflare (Pages or a Worker). Not StrixAlpha. Enterprise spec R22: this is vendor material, not customer content. |
| `https://pyrallon.ai/v1/connections/callback` | The OAuth relay callback the product spec names as the confidential web client's only redirect. This repo's connectors spec calls the same role `connect.pyrallon.ai`. Use the product URL as the one providers are given; a second hostname can point at the same Worker later. Staging, when it exists, is `connect.staging.pyrallon.ai`. | A Cloudflare Worker. Not the appliance, and not through the tunnel. The relay has to answer while StrixAlpha is unreachable, and it is the thing that holds client secrets so the appliance can stop holding them. |
| `https://magnus.pyrallon.ai` | The appliance people and the desktop app reach: its HTTPS API and the same UI the appliance already serves. | StrixAlpha, through the named tunnel. |
| `https://identity.pyrallon.ai` | Keycloak, realm `magnus`. Already the issuer. Renaming it to `auth.pyrallon.ai` would change every token's issuer and is not worth it (`docs/proposals/auth.md`). | Only move it onto this tunnel after confirming the name is served from StrixAlpha today. If the name already points somewhere else, leave its DNS alone. |

### What the relay does

The connectors spec (Relay, Connect flow) and the product registration
packet agree on the job, and disagree with a finished design of the handoff:

1. A provider redirects the person's browser to
   `https://pyrallon.ai/v1/connections/callback` with `code`, `state`, and,
   for Google, `iss`.
2. The relay checks `state` (which appliance, which person, which
   connector, a nonce, an expiry). It does not keep the code.
3. It sends the browser on. The connectors spec says: to the appliance's
   completion endpoint at that appliance's pyrallon.ai name, found from an
   appliance registry. `docs/proposals/spec-decisions.md` says that
   registry, and who runs it, is not specified. The line in the connectors
   spec is marked proposed, not ruled.
4. For a confidential client, the relay also proxies the token call
   (exchange, refresh, revoke): the appliance sends the code and the PKCE
   verifier, the relay adds the client secret, and it stores nothing. It
   logs appliance, provider, operation, time and result, never a code or a
   token. Appliances authenticate with the mutual-TLS certificate issued
   at provisioning. That certificate path is not built.
5. The browser page the person sees is "Connected, return to Magnus."

`workers/connections/oauth_worker.py` in magnus-product is not this relay.
It is the OAuth client that runs on a private pipe inside Magnus: it builds
the authorize URL, exchanges the code, refreshes and revokes. Its
`redirect_uri` is configuration. It does not listen on pyrallon.ai.

Why the relay is a Worker and not a process on StrixAlpha: the connectors
spec wants it stateless, in at least two regions, holding client secrets
and no user data, and talking only to providers' token endpoints. Putting
it on the appliance puts the secret back where the spec is taking it off,
and the callback dies when the tunnel dies.

Why it is not implemented in this pull request: the handoff is the part
that has to be right, and it is the part that is not ruled. A Worker that
accepts a `code` and forwards it would be an open redirect for authorization
codes. The current appliance cannot receive that forward anyway:
`POST /v1/connectors/{name}/sign-in/complete` requires the person's own
session, and the desktop app today catches the code on
`http://127.0.0.1:<port>/callback` and posts it itself. A browser that has
just come from Google has neither that session nor a paired device.
Building the Worker waits on a ruled state format and on the
appliance authenticating to the relay.

Until that exists, provider consoles keep the loopback redirect the desktop
app sends now. Do not switch them to the relay URL before the relay and the
appliance both speak it.

### The handoff, as the server speaks it (2026-11-13)

The server's half is built (`crates/magnus/src/connectors/relay.rs` and
`relay/`; `decisions/2026-11-13-relay-handoff-server-side.md`). It replaces
the browser bounce above: the relay never forwards a code, and the browser
never carries tokens.

1. Start. For a provider in `[relay] via_relay`, the server builds the
   provider address with `redirect_uri` the relay callback, its PKCE
   challenge, and `state` = `<server id>.<nonce>`. The nonce waits in
   memory for 15 minutes with whose sign-in it is. The provider sees no
   person and no connector.
2. Claim. The relay gets `code`, `state` and `iss`, finds the server by its
   id, and posts a signed `claim` (server id, nonce, `iss`, issue time) to
   `POST /v1/relay/claim`. The server answers, signed with its own key,
   with the provider, connector, person and PKCE verifier, once per nonce,
   and only when `iss` names the authorization server the sign-in went to.
   An unclaimed code is never exchanged, so the relay is not an inbox for
   codes.
3. Exchange and handoff. The relay exchanges the code with its secret and
   posts a signed `handoff` (server id, nonce, connector, person, tokens,
   issue time) to `POST /v1/relay/handoff`. The server checks the signature,
   the server id, that the nonce is claimed, unused and unexpired, the issue
   time within 120 seconds, and the connector and person; then keeps the
   tokens as any sign-in's and connects the app. Anything else: refused
   (403), the reason logged, nothing kept. The relay keeps no tokens.
4. Refresh and revocation. The server signs a `refresh` or `revoke` request
   to `POST <relay url>/v1/refresh` or `/v1/revoke`; the relay answers with a
   signed `refreshed` (tokens, or refused) or `revoked`.

Every message is `{payload, signature}`: the message JSON, base64url, and
an ed25519 signature over `magnus-relay-v1\n` and those bytes. The relay's
public key is configuration; the server's key is made on first start, kept
in its vault and held in memory from boot (so the kill switch, which seals
the vault, can still revoke), and `GET /v1/relay` shows the owner its public
half for the relay's registration.

Not built: the relay service itself, its registry of servers, the desktop
app waiting for the connection instead of a loopback callback, and the
mutual TLS between server and relay.

## 2. What on StrixAlpha is reachable

The appliance speaks HTTP/1.1 only (`crates/magnus/src/tls.rs`). A client
that negotiates HTTP/2 gets the connection closed. The tunnel must not
offer HTTP/2 to the origin.

| Service | Address on StrixAlpha | Through the tunnel? |
| --- | --- | --- |
| Appliance API and UI | `https://127.0.0.1:7700` (HTTPS on all interfaces, `tls_listen`). Certificate is the appliance's own, which browsers do not trust. | Yes, as `magnus.pyrallon.ai`. Cloudflare presents its own certificate to clients. The origin check skips verification of that self-signed certificate, and the origin protocol is HTTP/1.1. |
| Appliance plain HTTP | `http://127.0.0.1:7701` | No. Config tokens are accepted only on this listener, and only from loopback (owner's ruling 13). cloudflared connects from loopback, so publishing 7701 would make those tokens work from the internet. |
| Admin console | `https://192.168.0.50:7443` and `https://100.92.130.94:7443` (Tailscale). It refuses loopback, so a tunnel aimed at `127.0.0.1:7443` cannot reach it, and the spec forbids an `admin.pyrallon.ai` name. | No. Tailscale, or the LAN. |
| Keycloak, public | Whatever already answers `identity.pyrallon.ai`. The repo does not record that listen port. | Only if this host is already that origin. Paths `/realms/magnus/` and `/resources/` only; everything else 404, which is how the admin API stays off the public name. |
| Keycloak admin | `https://127.0.0.1:19446` | No. |
| Postgres | `127.0.0.1:54341` (the appliance). Test servers on 54340 stay local too. | No. |
| SSH | `192.168.0.50:22` | Not a public hostname. Optional private route, below. |
| Browser sidecar, search, the vault, cloudflared's own metrics | Loopback. Metrics, once set, `http://127.0.0.1:20241/ready`. | No. |

### Ingress

One named tunnel, remotely managed (hostnames set in the dashboard, not a
config file committed anywhere). Account
`3a1be1145e2314f8612a451e325e0218`, zone `pyrallon.ai`.

| Public hostname | Origin | Origin settings | Who may open it |
| --- | --- | --- | --- |
| `magnus.pyrallon.ai` | `https://127.0.0.1:7700` | No TLS Verify on. HTTP/2 to origin off. HTTP Host Header `magnus.pyrallon.ai`. | A device certificate from this zone's managed CA, checked at the edge (section 4). No Cloudflare account for the person. Magnus then asks for that person's own sign-in. |
| `identity.pyrallon.ai` | the public Keycloak port, found on the host, never `19446` | HTTP/2 to origin off. Add this row only after confirming the name is served from StrixAlpha. | No Access application and no device certificate. A login page behind either is a second login. Rate limit the hostname. Path allow only `/realms/magnus/` and `/resources/`. |
| (catch-all) | `http_status:404` | | |

Pushes from connected apps (`decisions/2026-11-09-app-push.md`) arrive at
`magnus.pyrallon.ai/v1/push/...` from the app's own servers, which hold no
device certificate. The client-certificate rule must skip that one path
prefix: `not starts_with(http.request.uri.path, "/v1/push/")`. The address
itself is the credential, and a wrong one answers 404.

Not in the ingress: `7443`, `7701`, `19446`, `54341`, `22`, and the apex
`pyrallon.ai` (that name is the site and the relay, not the appliance).

After the tunnel answers, the appliance config gains, and only these:

```toml
[admin]
tunnel_ready = "http://127.0.0.1:20241/ready"
public_hostnames = ["magnus.pyrallon.ai"]
```

Add `identity.pyrallon.ai` to `public_hostnames` only if that name is on
this tunnel. The console is never in the list. Operations › Network then
shows the tunnel; Health shows cloudflared.

The desktop app pins the appliance's self-signed certificate on a private
address (a LAN or Tailscale address, an IP, a `.local` name). It does not
pin `magnus.pyrallon.ai`: that name presents Cloudflare's certificate,
which the public roots already trust, and the app presents the device
certificate it was given at pairing. Do not put a Cloudflare token, a
service token, or a WARP enrolment in the app. Pinning stays for the
private address (`spec-decisions.md`, question 20).

### What sign-in already enforces

This is why the appliance's own API can sit on a public name once a device
certificate is required, and why several other listeners must not.

- A person signs in with a password (Argon2id) or through the identity
  provider. The session token is `mgs_` plus 32 random bytes, stored only
  as a SHA-256. The desktop keeps it in the OS keyring, one session per
  appliance address, and sends it again on later launches. Signing in is
  once per device, not once per connection.
- A wrong password for a person who exists is counted in memory
  (`sign_in.max_failures`, then `sign_in.lockout_minutes`). The count is
  not kept across a restart and is not per address. Unknown names do the
  same hash work and are not counted, so guessing names does not fill the
  table. Sign-ins that the provider has to start are also limited per
  address (`may_begin`, a ten-minute window).
- A browser session is an HttpOnly `SameSite=Strict` cookie. A change that
  arrives with the cookie and no bearer token must also send
  `x-magnus-client`, which another site's page cannot set.
- Config tokens from the appliance config work only on the plain listener
  and only from loopback. That check does not look at forwarding headers.
  cloudflared connects from loopback, so publishing port 7701 would make
  those tokens work from the internet. The tunnel's origin is 7700, which
  is TLS, so a config token is refused there.
- The owner's backup password needs a one-time code from `magnus
  sign-in-code` on the machine. A tunnelled request carries
  `Cf-Connecting-Ip` (or `X-Forwarded-For`), and that code is refused.
- The admin console listens on the LAN and Tailscale addresses, port 7443,
  and refuses a loopback peer, so a tunnel to `127.0.0.1:7443` cannot
  reach it. There is no `admin.pyrallon.ai`. Keycloak's admin API is
  `127.0.0.1:19446`. Postgres is `127.0.0.1:54341`. None of these is an
  ingress hostname.

### Device certificates

Cloudflare only carries the traffic. Magnus decides who the person is.
Each computer is paired once. Pairing is what gives it a client
certificate.

The certificate has to be one Cloudflare's own client-certificate CA
signed. A certificate the appliance signed itself does not make
`cf.tls_client_auth.cert_verified` true, and bringing our own CA is an
Enterprise feature. The zone is treated as Free or Pro, so the appliance
asks Cloudflare to sign a request, with a token that can write
certificates on this zone and nothing else.

The private key is generated in the desktop app. The app sends only the
certificate request (`POST /v1/devices`, while signed in). The appliance
does not store the key and does not return one. The request is refused
when it arrived through the tunnel, so the answer is not a response
Cloudflare's edge can read: pair on the LAN or Tailscale, then use
`https://magnus.pyrallon.ai`. The first computer cannot pair through that
name once the edge requires a certificate, because it does not have one
yet. Leave Access in place until at least one computer is paired and the
edge rule below is on; then remove Access. While Access is still on and
the edge rule is not, the name is not yet held to a device certificate.

The appliance stores, in `state/accounts/devices.json` (mode 0600, beside
the sessions, not in the log): the device id, the person who paired it,
the name, Cloudflare's certificate id, the SHA-256 fingerprint, and the
serial. Revoking (`DELETE /v1/devices/{id}`, by that person or an owner)
deletes the certificate at Cloudflare, marks it revoked here, and ends
every session bound to that device. A lost computer stops at the edge.
The log records that a device was paired or revoked, not the certificate.

At the origin, a request is treated as the tunnel only when the peer is
loopback and `Cf-Connecting-Ip` is set, which is cloudflared. Such a
request must carry `x-magnus-client-cert` equal to a live device's
fingerprint (the header a Transform Rule sets, overwriting anything the
client sent). `GET /v1/health` is exempt so a probe can see the process;
the edge still requires a certificate before an internet probe arrives.
A request from the LAN is unchanged and does not need a device
certificate. The header is ignored from any peer that is not the tunnel,
because the client can set it. The edge rule is what a revoked
certificate cannot get past: `cf.tls_client_auth.cert_verified` stays
true for a revoked certificate, so the rule has to mention
`cf.tls_client_auth.cert_revoked` as well.

A session opened through the tunnel is bound to the device. The session
that did the pairing, on the LAN, is bound too. Revocation ends both.
The desktop keeps the public name's session in the keyring under that
address, separate from the LAN session. Opening the app again uses the
saved session.

The app will not call `POST /v1/devices` on behalf of the page, and the
page's general API cannot name that path, so the certificate and the key
never enter the page. A browser signed in on the public name has to have
the certificate installed in the browser or the operating system; the
desktop app is the client that stores it.

## 3. Runbook for what is already done

The tunnel, the hostname, and the Access application are already in place.
They were not created by this change. The install below is the record of
how the connector was put on the host; do not run it again if
`cloudflared` is already the system service.

```sh
# The tunnel install token. It is not written here, and it is not the
# certificate token in section 4.
curl -fsSL https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o /tmp/cloudflared
sudo install -m 755 /tmp/cloudflared /usr/local/bin/cloudflared
sudo cloudflared service install -- '<TUNNEL_TOKEN>'
```

Metrics, which the token install does not turn on:

```ini
# /etc/systemd/system/cloudflared.service.d/metrics.conf
[Service]
Environment=TUNNEL_METRICS=127.0.0.1:20241
```

```sh
sudo systemctl daemon-reload
sudo systemctl restart cloudflared
curl -fsS http://127.0.0.1:20241/ready
```

`/ready` returns 200 when the connector is registered. Port 7700 stays
reachable from the LAN and from localhost, and not from the internet: an
open forward to 7700 would walk around the edge rule and hit the
appliance with no device certificate. The LAN is allowed to do that on
purpose. The internet is not.

Optional private route, for SSH, not for the console, and not required
for the desktop app. Networks → Routes → Add `192.168.0.50/32` on tunnel
`strix-alpha`, with an Access application of type Private network for
`192.168.0.50:22` and the one email that may use it. That device runs
WARP. Do not publish port 22 as a hostname. Do not put the console behind
that route.

Identity, only if `dig +short identity.pyrallon.ai` is this host or is
already a name you intend to move. Find the public Keycloak port (`ss
-lptn`) and add that hostname to the same tunnel. Never use
`127.0.0.1:19446`. A rule returns 404 except for `/realms/magnus/` and
`/resources/`. No Access application and no device certificate on it.

## 4. What to change in Cloudflare, and the new secret

Do this after the appliance is running the build that pairs devices, the
config below is set, and at least one computer has paired from the LAN.
Until the rule is in place, leave the Access application where it is.
This change does not call the API.

### Token

Create an API token, not an all-accounts token. Zone `pyrallon.ai` only.

| Resource | Permission | Used for |
| --- | --- | --- |
| Zone `pyrallon.ai` | SSL and Certificates: Write | Sign a certificate request and delete a certificate. This is the narrowest permission the client-certificate API documents. It can also upload an origin certificate on this zone; it cannot edit DNS, Workers, Access, or any other zone. |
| Zone `pyrallon.ai` | Zone: Read | The call names the zone. |

Do not grant Account-level SSL, Access, Tunnel, or DNS on this token. The
tunnel token from section 3 is a different credential and stays in the
`cloudflared` unit, not in the vault.

Put the token text in the appliance vault under the name `cloudflare_mtls`
(the `secret` command, or the console's secret field). It is the raw
token, not a JSON blob. It does not go in git or in GitHub Actions.

The zone id is on the zone overview. It is not a secret. It is not in
this repository; paste the one the dashboard shows. Do not invent one.

### Appliance config

```toml
[client_certs]
hostname = "magnus.pyrallon.ai"
zone_id = "<zone id from the dashboard>"
credential = "cloudflare_mtls"
# days = 365

[admin]
tunnel_ready = "http://127.0.0.1:20241/ready"
public_hostnames = ["magnus.pyrallon.ai"]
```

`credential` and `days` have those defaults. `days` must be from 1 to
3650, which is what Cloudflare's API accepts. Restart the appliance after
the config and the vault secret are in place. Pair from the desktop app
by signing in on the private address: the app asks for a certificate for
`https://magnus.pyrallon.ai` and keeps the key in its own file, mode
0600. Then sign in once on that public address. Later launches reuse that
session.

Add `identity.pyrallon.ai` to `public_hostnames` only if that name is on
this tunnel. The console is never in the list.

### Edge

API host `https://api.cloudflare.com/client/v4`. Replace `ZONE` and
`TOKEN`. These calls are the ones to run; the dashboard path is under
each.

1. Remove the Access application on `magnus.pyrallon.ai` only after the
   block rule in step 3 is enforcing. Access → Applications → the Magnus
   app → Delete. While you are pairing the first computers, leave it.

2. Turn on client certificates for the hostname. Dashboard: SSL/TLS →
   Client Certificates → Hosts → Add, host `magnus` (the apex
   `pyrallon.ai` is appended). Create a Cloudflare-managed CA if the zone
   does not have one. Do not upload your own CA.

   The API that enables the host is under the zone's client-certificate
   settings. Hosts are also editable as `client_certificate_v2` in the
   zone settings. Enabling the host makes Cloudflare request a
   certificate. It does not, by itself, reject a missing or revoked one:
   `cert_verified` is true for a revoked certificate. Step 3 is the
   rejection.

3. A WAF custom rule, action Block:

   `(http.host eq "magnus.pyrallon.ai" and not cf.tls_client_auth.cert_verified) or (http.host eq "magnus.pyrallon.ai" and cf.tls_client_auth.cert_revoked)`

   Dashboard: Security → WAF → Custom rules → Create. The same expression
   is the `expression` field of
   `POST /zones/ZONE/rulesets/phases/http_request_firewall_custom/entrypoint`
   with action `block`. A Free plan has few custom rules; if the zone
   already uses them all, say so before deleting Access. Without this
   rule a revoked certificate still passes the edge.

4. A Transform Rule that sets the request header, overwriting a value the
   client sent. Dashboard: Rules → Transform Rules → Modify request
   header. Header name `x-magnus-client-cert`, value
   `cf.tls_client_auth.cert_fingerprint_sha256`, operation Set.

   `PUT /zones/ZONE/rulesets/phases/http_request_late_transform/entrypoint`
   with a rule whose action is `rewrite` and
   `headers.x-magnus-client-cert.operation` is `set` and
   `headers.x-magnus-client-cert.expression` is
   `cf.tls_client_auth.cert_fingerprint_sha256`. Match
   `http.host eq "magnus.pyrallon.ai"`.

5. Confirm, from a computer that has not paired, that
   `https://magnus.pyrallon.ai/v1/health` is blocked at Cloudflare and
   never reaches StrixAlpha. From a paired computer the same URL is the
   appliance's health JSON. From the LAN, the private address still works
   with the pinned certificate and no device certificate.

The appliance calls, with the vault token, `POST
/zones/{zone_id}/client_certificates` with `{"csr", "validity_days"}` and
`DELETE /zones/{zone_id}/client_certificates/{id}`. A zone on this plan
holds about 100 active client certificates; revoking frees a slot. The
appliance stops a person at 20 live devices, and at 8 pairings in an
hour, before that quota. The hour's count is in memory and starts over
when the appliance restarts.

### Risks that remain

- The first computer has to pair on the LAN or Tailscale. After the block
  rule is on, an unpaired computer cannot open the public name to pair
  itself.
- Without the block rule, a revoked certificate still has
  `cert_verified` true and the edge lets it through. The origin then
  refuses it only if the Transform Rule overwrote the fingerprint header.
  A client who can set that header, and who still has some other live
  certificate, can name a different device. Set the header, do not set it
  only when it is missing.
- A process on StrixAlpha can connect to loopback and set both
  `Cf-Connecting-Ip` and a live fingerprint. That is the same position as
  already being on the machine. The LAN can reach port 7700 with no
  device certificate; that is intentional. A port forward of 7700 from
  the internet would skip the edge.
- The working list of devices is the file, not the log. If the file is
  lost, Cloudflare still has the certificates until they are revoked in
  the dashboard. The log says that a device was paired or revoked, not
  the fingerprint.
- A browser on the public name does not store the key the way the desktop
  app does. It needs the certificate installed in the browser or the
  operating system.
- The public name's session is a different keyring entry from the LAN
  session. Signing in on one does not sign in on the other. Each is once,
  then kept.
- Lockout of a wrong password is still per person, in memory, and not per
  address. A paired stolen computer can try passwords until the lockout,
  and the count does not survive a restart.

### Redirects to switch, later

Not now. When the relay and the appliance both implement the handoff, the
confidential clients move from `http://127.0.0.1:<port>/callback` to exactly:

`https://pyrallon.ai/v1/connections/callback`

That is one URL for every provider. The connectors spec's
`connect.pyrallon.ai` is the same callback on a second name, not a second
path. Google's current client is a Desktop client, which accepts any
loopback port and does not use this URL; the product spec's web client is
the one that will. Switching Google before the relay answers makes sign-in
fail. Slack, Vercel, Sentry, Asana, GitLab, Hugging Face, Supabase and
Cloudflare, which require an exact redirect, are the ones this URL unblocks.
GitHub can stay on `http://127.0.0.1/callback` until the same cutover.
Microsoft's Entra redirect list gains this URL and drops the loopback one
at the same time.

## 5. Where the relay code goes

Not in this repository yet.

The device-certificate path above is the mutual TLS between a person's
computer and `magnus.pyrallon.ai`. It is not the mutual TLS the connectors
spec describes between an appliance and the relay. That one is still
unbuilt. The handoff is now decided and its server half built (section 1,
"The handoff, as the server speaks it"): the relay exchanges only codes
whose nonce a server signed a claim answer for, so it is not an open
redirect for authorization codes.

The Worker is small only after the state format is ruled. What is ruled is
the role (bounce the callback, proxy the token call, hold the secret, store
no user data, log no code). What is not ruled is how `state` names an
appliance and how the code reaches `POST /v1/connectors/{name}/sign-in/complete`
without becoming a public inbox for authorization codes. magnus-product
already has the in-process OAuth client (`oauth_worker.py`); it is not a
public edge program, and copying it into a Worker would put token handling
on the edge without the private pipe it assumes.

The site (homepage, privacy, terms) is also unwritten. Publishing a Worker
that returns placeholder policy pages would be the guessed text the
registration packet says not to submit.

Next code is the Worker, in its own repository or under `relay/` here: the
callback, the server registry (server id, its address, its public key), the
claim, the exchange with the secret, the handoff, and the refresh and revoke
endpoints, speaking the messages in `connectors/relay.rs`. On the server
side what remains is mutual TLS to the relay and the desktop app waiting for
the connection (the sign-in's answer names no loopback callback). Then each
provider moves over: its console's redirect switched to the relay callback
and its name added to `via_relay`.
