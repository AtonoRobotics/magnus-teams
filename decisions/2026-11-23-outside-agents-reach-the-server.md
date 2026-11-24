# How an outside agent reaches the server without a device

**Choice.** `require_device_cert` lets a tunnel request through only from a
paired device. An outside agent has no device. Three paths are exempt from
that rule, and only these: `GET /.well-known/agent-card.json`,
`GET /.well-known/agent-card-keys.json` and `POST /a2a` (`a2a::PUBLIC_PATHS`,
checked in `devices::admit`). The two `GET`s publish only what is public by
design: the signed card and its public keys. `POST /a2a` never takes a
session or a cookie. Every request must carry the per-agent key the card
declares. The key is compared by digest with the vault's copy and belongs to
a connected agent. The agent's Cedar principal must admit the person or
project the request names. All of this happens before anything is created
or run. The body is limited to `MAX_REQUEST_BYTES` (64 KiB) and each agent
to `MAX_REQUESTS_PER_AGENT_PER_HOUR`. A refusal creates nothing. Inbound
mutual TLS is not offered: the origin terminates no client certificates of
its own, so the card declares the API-key scheme alone.

The Cloudflare edge in front of the tunnel blocks every request without a
verified client certificate (the WAF rule in
docs/remote-access-and-relay.md, step 3). It must let these three paths
through without one (step 6 there gives the expression). That is a host
change, so it is the owner's to make. Until it is made,
outside agents can reach the server only where the edge does not stand in
front of it (the LAN, or a deployment without the mTLS rule).

**Reason.** The design says an outside agent is a principal, never a
person, and that a request is authenticated with the scheme the card
declares and checked against its principal. A device certificate would make
it a person's machine, which it is not. Exempting the whole API, or letting
a session through, would widen what an unpaired caller can reach. Exempting
exactly the card, its keys and one endpoint that authenticates every call
itself is the narrowest opening that lets the feature work at all.
