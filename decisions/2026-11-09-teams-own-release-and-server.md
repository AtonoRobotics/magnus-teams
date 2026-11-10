# magnus-teams has its own release key, update service and server

magnus-teams signs its releases with its own key, `~/.magnus-teams-release/update.key` on magnus-server, its build box. Its public half replaces `release/update.pub` and the desktop app's updater key. Those were inherited from magnus-claude and are no longer trusted here. The scripts default to the new path, and `MAGNUS_RELEASE_KEY` still overrides it.

Its appliance on magnus-server is its own: an install directory, state, database, ports, TLS and device certificates, and an updates directory that it installs from by itself. The Postgres installer defaults to its own container, volume and port (`magnus-teams-pg`, `magnus-teams-pgdata`, 54351), so it never reuses magnus-claude's.

Reason: the owner said that magnus-claude is not the same app (2026-11-09). A shared key would let one app's release install as the other's, and a shared database or appliance would mix two apps' users and data. magnus-claude's appliance on this machine, `~/.magnus-appliance`, is left alone.
