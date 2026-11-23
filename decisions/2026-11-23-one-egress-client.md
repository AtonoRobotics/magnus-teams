# One way to reach an address someone else chose

**Date:** 2026-11-23

## Decision
`crates/magnus/src/egress_client.rs` is the only place the server builds an
HTTP client for an address someone else chose. It resolves the host once,
refuses it when any address is private (`net_guard`), unless
`[web] allow_private_networks` is set, and pins the connection to the
addresses it checked. No client it builds follows a redirect by itself:
`get` follows up to five hops, each checked like the first; `send` follows
none.

Its callers: an added app's MCP calls (`apps::Mcp`, open and every RPC),
connector discovery (initialize, the app's name, protected-resource and
authorization-server metadata), client registration, the token exchange and
refresh, revocation, the sign-in page probes, generated connectors and the
agent's web fetch. `connectors::http()`, a default client that followed
redirects and checked nothing, is gone.

Redirects: the GETs that read metadata and web pages follow checked hops.
Every POST (initialize, registration, token, revocation, MCP calls) follows
none: the protocols need none, and a 307 would replay a code or refresh
token to wherever it points. A provider whose token endpoint redirects now
fails the sign-in instead of sending the token on.

Left alone, as fixed addresses the operator configured (each named in the
allowlist of `ci/egress-check.sh`): the identity provider (`oidc.rs`, set by
the server admin), Cloudflare for device certificates, the embeddings
server, Link (`payments/link.rs`: its API and login addresses come from the
config, and nothing it answers is fetched), Microsoft Graph and Google for
push subscriptions, the image provider, the relay and the search service
(both through `egress_client::configured`), the model adapters, and the
desktop app and local program reaching their own server.

`ci/egress-check.sh`, run by `ci/fast.sh`, fails when a client is built in
any other file, and when an allowlisted file no longer builds one.

## Reason
A remote app chooses its own resource metadata, authorization server,
registration and token addresses, and the server fetched and posted to them
with a client that followed redirects and checked nothing, the person's
refresh token included. A malicious app could point them at the cloud
metadata address or a port on the server. Web fetch, generated connectors,
brokered servers and the computers' proxy already checked and pinned; one
module makes every path do the same, and the check keeps it so.
