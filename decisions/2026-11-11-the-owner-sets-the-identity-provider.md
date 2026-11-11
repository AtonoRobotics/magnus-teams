# The owner sets the identity provider in Admin › Security

Spec rev 141, Administration › Security: "Sign-in policy (identity provider,
two-factor, session length)". Security showed the provider from the config
and could not change it.

Choice: the provider is set in Security, as `crates/magnus/src/admin_app/identity_provider.rs` describes:

- **Only an owner sets it.** A provider says who anyone is, the owner
  included, so whoever picks it can make themselves the owner. An admin who
  could set it could take the server. The spec's Security section is for
  owners and admins, and this is the safer reading of it.
- **Validated first.** Only the settable fields are taken: issuer, client
  id, the person/groups/roles claims, scope, label, and the status client
  with its vault secret and admin address. The issuer and admin address
  must be https, or on this machine. A status secret has to be in the vault
  (secrets come from the vault only). A certificate-authority file path and
  the sign-in limits stay in the config: the app does not name files on the
  server.
- **A test sign-in before the switch.** The owner signs in through the new
  provider on its own page (the device-code grant, which every provider
  Magnus supports offers, with no status client needed). The test passes
  only when the provider names the owner making the change. The switch
  takes exactly the settings that were tested, within
  `IDP_TEST_VALID_SECONDS` (10 minutes), and only once. A provider that
  would sign the owner in as someone else, or not at all, is refused.
- **Never locking out the last owner.** With a provider, the owner's way in
  is the one just tested, and their backup (a password here and a code
  printed on the server's machine) stays as before. Turning the provider off
  (passwords kept here) needs the owner making the change to have a password
  here. Whether a password signs someone in now follows the provider in use
  (`Kernel::may_use_password`), not only the config's.

The setting is a new variant (`IdentityProvider`) of the existing
`admin_setting` event, so no event kind changes. It is audited before it is
made (`identity_provider_set`, before and after, no secrets: the status
client's secret is only named), takes the admin step-up, and takes effect at
once. A server starting again builds the provider from it. If it no longer
builds (its secret removed, say), the server uses the config's provider and
logs why, rather than refusing to start.
