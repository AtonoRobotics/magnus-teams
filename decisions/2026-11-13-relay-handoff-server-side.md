# The relay handoff, server side (GAP6, step 1)

**Choice.** The server can complete a confidential client's sign-in through
the relay without holding the client secret. It is off unless `[relay]` is
configured, and today no server has one. Code: `crates/magnus/src/connectors/relay.rs`
(settings, keys, messages), `relay/sign_in.rs` (start, claim, handoff,
routes), `relay/client.rs` (refresh and revoke), `relay/tests.rs` (a fake
relay).

- **Registration.** `[relay]` holds `server_id`, the relay's ed25519
  `public_key`, its API `url`, the `callback` (default
  `https://pyrallon.ai/v1/connections/callback`) and `via_relay`, a list of
  catalog providers. All of it is checked at startup: the id must fit
  `state`, the key must be 32 bytes, the addresses must be https (plain http
  only on loopback, for tests), and each provider must take a secret.
  Without `[relay]` the routes answer 404 and sign-in is exactly as before.
  `via_relay` names providers, not connectors: catalog connectors are not in
  the config file, and the relay holds one secret per provider client.
- **The server's own key.** It is made on first start and kept in the
  vault (`relay_server_key`). It is held in memory from boot so the kill
  switch, which seals the vault, can still sign revocations. `GET /v1/relay`
  (owner only) shows its public half for the relay to register. GAP6 said
  the relay issues the key at registration. Making it on the server is
  safer: the private key then exists in one place, and the relay needs only
  the public half.
- **Start.** The redirect is the relay callback and `state` is
  `<server id>.<nonce>`. The nonce waits in memory with the same 15-minute
  expiry as other sign-ins (a restart loses it, as today). The person and
  connector stay on the server, so the provider never sees them.
- **Claim, an added step.** The relay posts a signed `claim` (server id,
  nonce, `iss`, issue time). The server answers, signed with its key, with
  the provider, connector, person and PKCE verifier. Each nonce is claimed
  once, and only when `iss` names the server the sign-in went to (RFC
  9207). Why the step: the relay must have the verifier to exchange a PKCE
  code. Putting the verifier in `state` would defeat PKCE, and keeping it at
  the relay would make the relay stateful. With the claim, the relay
  exchanges only codes a server vouched for, which answers the "open
  redirect for authorization codes" objection in
  `docs/remote-access-and-relay.md`. The relay checks `iss` against the
  server's metadata by passing it on; it never trusts its own guess.
- **Handoff.** The relay posts a signed `handoff` (server id, nonce,
  connector, person, tokens `{access_token, refresh_token?, expires_in?,
  scope?, account?}`, issue time). The server checks, in order: the
  signature, the server id, the issue time within `ISSUED_SKEW_SECS` (120 s),
  that the nonce is waiting, unexpired and claimed, that connector and
  person match, that the kill switch is off, and that the app still signs in
  through the relay. Then it keeps the tokens through the same code a direct
  sign-in uses (`fresh_tokens`, `keep_sign_in`, then `open_after_connect`
  and `signed_in_after_consent`). Any handoff the relay signed uses the
  nonce up, even one that is refused. A forged one does not, so a stranger
  cannot cancel someone's sign-in. Every refusal answers a plain 403
  "refused", logs its reason, and keeps nothing.
- **The relay keeps no tokens: the contract.** It normalizes the provider's
  answer (Slack: the person's user token, not the bot's) and forgets it once
  the handoff is answered.
- **Refresh and revocation.** The server sends a signed `refresh` or
  `revoke` to `<url>/v1/refresh` or `/v1/revoke`, with a fresh request id.
  The relay answers with a signed `refreshed` (tokens, or refused) or
  `revoked`, naming the same request. A refused refresh marks the sign-in
  lapsed, as a provider's refusal does. An unreachable relay leaves the
  sign-in alone, so it keeps working until its access token expires. The
  stored client records `relay = <provider>`, so a relayed sign-in is
  refreshed and revoked through the relay even if `[relay]` later changes.
  The `RelayClient` trait has one implementation, over HTTP.
- **Messages.** `{payload, signature}`: the message's JSON in base64url,
  signed with ed25519 over `magnus-relay-v1\n` followed by those bytes. The
  signature covers the exact bytes, so no canonical JSON is needed, and the
  context prefix keeps a signature made for anything else from verifying.
  `ring` provides ed25519; it was already a dependency.
- **Client ID.** A relayed sign-in uses the client ID provisioned on the
  server, else the catalog's (`registrations::client_id`). The relay must
  hold that client's secret.

**What waits for the relay service:** the Worker itself (callback, server
registry, claim, exchange, handoff, refresh, revoke); mutual TLS between
server and relay; the desktop app waiting for the connection instead of a
loopback callback; and moving each provider over (its console's redirect to
the relay callback, its name into `via_relay`). Until then the
admin-provisioned secret path is unchanged.
