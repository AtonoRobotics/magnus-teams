# #211: recipient provenance from the organisation's directory

The spec's third and last provenance source: under both taints, a send
to an address the realm holds passes with no card. Own words (#176)
and sent mail (#210) stay the first two sources.

## Shape

A new `Identity` method, `in_directory`: the Keycloak admin API's
users by exact email, asked as the status client, mirroring the
existing account lookup. Whatever stops the read counts as not
there — the gate asks rather than passing on an unchecked claim —
and with no provider configured there is no waiver. No caching: the
check runs only for tainted sends to unnamed addresses, rare enough
that one directory read each is proportionate.

## Test seam

The e2e stubs the whole provider in-process (discovery, token,
admin users) and points `[sign_in.oidc]` at it, seeding the status
secret over the owner's secrets route — no Keycloak needed, so the
test runs where #312's missing CA breaks the container ones.
