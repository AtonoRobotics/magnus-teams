# Backups hold the databases the appliance depends on (2026-11-06, #296)

- New: `[backup] databases`. Each backup dumps those databases into the encrypted file, its restore check reads each dump back, and `magnus restore` puts them back.
- On strix-alpha, once this release is on the appliance: `[backup] databases` names the `keycloak` database (#290), so the realm, its clients and everyone's accounts are in every backup.
- Next: #297 copies the backups to strix-beta, off the appliance's disk.
