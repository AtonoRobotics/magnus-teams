# Settings holds no administration; every admin change takes the step-up

Spec rev 141, Administration: "Administration lives in the app, not in a separate console... Each admin change asks for the admin's password and code again if they have not confirmed them in the last 5 minutes."

An audit found the app's Settings still changing the server's models and providers, the organisation's connectors, people's roles, the server's updates, backups and the kill switch, through owner routes outside `/v1/admin` with no step-up and no admin audit entry. Two changes close it.

## Settings is a person's own

Settings keeps Account, Rules, Payments, Appearance, Notifications, Updates (this app's version, and the server's version read-only) and Data and privacy. Its Models, Connectors and Kill switch sections, the roles table, the server's Install button and the backups card are gone. Their old addresses (`#/settings/models`, `#/settings/connectors`, `#/settings/safety`, `#/models`, `#/apps`) open Admin for an admin. An admin sees one line in Settings pointing to Admin.

- Connecting model providers (keys and sign-ins into the vault, the conformance suite) is drawn inside Admin › Models, for the owner, whom the server lets do it.
- What the kill switch revoked and sealed, and what to connect again, is a card in Admin › Overview. Pressing and releasing are in the top bar.
- Roles are Admin › People's; backups and restore checks, install and roll back are Admin › Operations'; the organisation's connectors are Admin › Connectors'.

## The old routes cannot bypass the step-up

The routes stay, so a desktop app from before this release still works, but a layer on the whole app router (`admin_app::settings_gate`) holds each of them to the Admin section's rules when an owner or admin calls it: a change needs a step-up within 5 minutes (`403 {"error":"step_up"}` otherwise) and is written to the admin audit first (no entry, no change; a refused change is recorded as `<action>_failed`). The table of routes is `ADMIN_CHANGES`; a test fails when one of them is not gated or a person's own route is. The server's kill switch follows the Kill switch rule there too: pressing needs no step-up and is recorded after it holds, releasing needs one and is recorded first. The app's `api()` opens the step-up sheet for any change the server holds this way and sends it again, so the step-up works wherever a change is made.

A key from the config file (`[[people]] token`) needs no step-up, though it is audited the same. Such a key works only on the server itself over its local address (`config_tokens_on_this_machine`), never from the internet the step-up guards against, and whoever holds it already holds the config file. The deployments and test harnesses that call these routes use such keys.

Not covered here, and left as they are: an owner signing out another person's device (`DELETE /v1/devices/{id}`) and trashing someone else's item. Both act on one person's things rather than the server's settings; Admin › People's own session controls already take the step-up.
