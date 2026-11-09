# The last admin stays when two changes come at once (2026-11-06)

Demoting, turning off, deleting and cancelling an admin's invitation in the admin console now hold one lock from the last-admin check, read from the provider under it, until the provider has made the change, so two changes aimed at the last two admins can no longer both go through. A new test against a real Keycloak sends such pairs together in twelve rounds and fails on master in the first.
