# The admin console is retired

Spec rev 141 (Administration): "The separate VPN-only console built for the retired appliance is retired, except that magnus kill on the server itself still stops everything when the app or sign-in is broken." The owner added that the console's Knowledge section goes with it.

## What goes

- `magnus-admin` (`crates/magnus/src/bin/magnus-admin.rs`) and its server, sign-in, certificates, tunnel and network screens (`crates/magnus/src/admin/`), and its page (`desktop/admin/`).
- The Knowledge section: `crates/magnus/src/knowledge_admin.rs` and the control routes `/v1/admin/knowledge*`. The knowledge itself, and the app's own routes to it, stay.
- The control routes only the console called: `/v1/admin/console-sign-in`, `/v1/admin/console-standing/{person}` and `/v1/admin/console-settings`. The app's Admin section has its own guard and step-up (`decisions/2026-11-11-admin-in-the-app.md`).
- Packaging: `magnus-admin` leaves `release/appliance-programs`; `release/admin-on-appliance.sh` and `release/admin-section.py`, which set the console up on a server, are deleted; `config/keycloak-client.py` no longer registers the console's client and service account.
- The scratch tests that drove the console (`tests/scratch/tests/admin_*.rs`, `tests/scratch/src/admin.rs`, `desktop/tests/admin-sign-in.mjs`). What they proved about people, sign-in rules, connectors and updates is the control routes' behaviour, and it is tested again through the app's Admin routes (`crates/magnus/src/admin_app/`).
- The crates only the console used (`hyper`, `hyper-util`, `http-body-util`).

## What stays

- The control socket and `magnus kill`, which reach the kernel without the app or sign-in.
- The control routes the app mounts in-process, so an admin change has one implementation and one audit entry.
- The `ConsoleSettings` and `AdminAction` event kinds and their readers: an event kind is never deleted. `ConsoleSettings` now changes nothing anyone reads.
- `[admin]` in `magnus.toml`. A deployed server's config still has it, and a config that stopped loading would stop the server. It is read as any table, ignored, and the server logs a warning naming it. `magnus migrate` no longer creates the console's `magnus_root` role, whose only user was the console; a role made before stays as it was, unused.

## Why now

The app's Admin section covers every console screen the spec keeps (Overview, People, Connectors, Rules, Security, Operations, with Agents, Models and Spend built beside them), so nothing an admin does needs the console. Keeping a second door that reads past the walls between people, on a separate network path, is risk with no use.
