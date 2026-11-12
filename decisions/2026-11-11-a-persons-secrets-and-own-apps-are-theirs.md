# A person's own secrets and the apps they build are theirs, not the server's

Spec rev 145: "Each user has their own VM, their own key in the one credential service"; Generated connectors: "A connector Magnus builds is kept as files in the user volume and listed with the user's other connectors. The user can download its definition or delete it." Connectors spec: each person signs in with their own account, "and no one else acts through it". The owner, 2026-11-11: "users should be isolated from each other securely".

An audit found that the owner's vault route and the admin's connector routes reached into these:

- `PUT /v1/secrets/{name}` wrote any vault name, so the owner could put their own account's token under a person's sign-in (`connector_<app>__<person>`), card, Link wallet, browser identity or computer credential. The person's agent would then read and send through the owner's account. `GET /v1/secrets` listed them all. Now those names (`secret_scope::personal`) are refused (403) and not listed; the server's own secrets (a mailbox password, a provider's key, an organisation app's key) are set as before.
- A connector a person built (`owner: Some(person)`) was treated as an organisation app by `DELETE /v1/connectors/{name}`, `PUT /v1/connectors/{name}/tools/{tool}`, the Admin connector list (which carried its whole definition in `url`), and the Admin credential, revoke and on/off routes. Now those see only the organisation's apps (`org_connectors`, and `added_connector` refuses an owned one), so a person's own app is changed and deleted by that person alone, through `generated.rs`.

Admin keeps "who has connected what" (spec, Connectors): each organisation app's row still counts the people signed in to it.
