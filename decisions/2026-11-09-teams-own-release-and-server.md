# magnus-teams has its own release key, update service and server

magnus-teams signs its releases with its own key, `~/.magnus-teams-release/update.key` on magnus-server, its build box. Its public half replaces `release/update.pub` and the desktop app's updater key. Those were inherited from magnus-claude and are no longer trusted here. The scripts default to the new path, and `MAGNUS_RELEASE_KEY` still overrides it.

magnus-teams runs on its own server, magnus-server, at `~/.magnus-teams`: its own state, database, ports, TLS and device certificates, and an updates directory that it installs from by itself. The Postgres installer defaults to its own container, volume and port (`magnus-teams-pg`, `magnus-teams-pgdata`, 54351), so it never reuses magnus-claude's.

What people see says "server", as the spec does. "Appliance" is magnus-claude's word. Code names and wire names that say appliance (`x-magnus-appliance`, `release/to-appliance.sh`) stay, because renaming them would break compatibility for no gain to anyone using the app.

Reason: the owner said that magnus-claude is not the same app (2026-11-09). A shared key would let one app's release install as the other's, and a shared database or server would mix two apps' users and data. The magnus-claude appliance on this machine, `~/.magnus-appliance`, is left alone.
