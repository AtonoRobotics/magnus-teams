# The connector catalog

One manifest per connector that ships in Magnus (connectors spec, "Catalog
and connector manifests"). They are compiled into the appliance, so each
release's signature covers them; a separate signed catalog service replaces
that when Alpha Vector's catalog key and signing service exist.

A manifest names the connector's server, how people sign in to it, the
access it asks for, and the class of every tool it has: that class counts as
the organisation owner's classification (owner's ruling 5). A tool the
server lists that its manifest does not name stays hidden until a new
manifest version classifies it.

`registration` is how Magnus is known to the provider:

- `dcr`: Magnus registers itself with the provider's server (nothing to set up).
- `preregistered`: Alpha Vector's own client with that provider; its client
  ID and secret are the organisation's registration for `provider`, set
  once by the owner (Settings, Connectors), kept in the vault. The connector
  is in no one's Library until it is set.

Classes: `read`, `reversible_write`, `irreversible`, `financial`, and
`prohibited` (listed by the server, never offered to Magnus and refused
if called: the policy's hard deny).
