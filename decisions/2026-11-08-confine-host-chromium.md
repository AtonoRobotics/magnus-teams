# Confine the host Chromium in a bubblewrap jail (2026-11-08)

Chromium reads hostile pages on the machine that holds the vault, and it ran with `--no-sandbox`.

- Chromium runs inside a bubblewrap jail. It sees `/usr`, its own install and a private work directory, and has no network of its own. Reason: a page that exploits the browser must not reach host files or the host's network.
- Chromium's own sandbox stays on inside the jail. Reason: renderers then have their own namespaces and a seccomp filter, so a page has two layers to break.
- The only way out of the jail is the existing egress proxy, reached over a Unix socket through a relay on the jail's loopback. Reason: Chromium needs a TCP proxy address and the jail has no route to the host.
- The jail uses its own copy of bubblewrap at `/usr/local/lib/magnus/bwrap`, `root:magnus` mode 0750, installed by `config/install-browser-jail.sh`. Reason: only Magnus processes may use it.
- Where AppArmor restricts unprivileged user namespaces, as Ubuntu 24.04 does, an AppArmor profile allows them for that copy only. Reason: the system's own bwrap and every other program stay restricted.
- The browser refuses to start without the jail, with no unconfined fallback. Reason: a fallback would put the old exposure back without anyone noticing. Every machine that runs the browser needs the script, and Magnus needs a restart to pick up the group.
