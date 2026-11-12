# The agent runs commands and installs apps on its own computer

The owner: "magnus computer file system is read only and it claims it can't install programs to its own computer, which violates the spec."

Cause: the agent's only shell (`sandbox_exec`) and file tools run in the per-run sandbox, a microVM on the read-only base image with no writable layer and no network. Only the desktop tools (`computer_click`, `computer_type`, ...) reached the person's computer, whose system is a writable overlay (checked on a live computer: `/` is an overlay, `/usr`, `/opt` and `/etc` are writable, the guest runs as root). So the agent had no way to change or install anything on its computer.

Spec: "Layer ... App manifest: Every app the agent installed: name, version, source, hash, install steps"; "Boundary actions. ... an install"; "Reads, and everything inside the computer, are not boundary actions and pass without review."

Choice:
- `computer_exec` runs a shell command as root on the run's own computer (made ready and woken as the desktop tools are). It is inside the computer, so it is class `sandbox` and not reviewed; its network goes through the computer's egress proxy as everything on the computer does.
- `computer_install_app` installs an app the agent downloaded with `computer_exec`: it runs the steps with the file as `$INSTALLER` (the name replay gives the cached copy), then records the manifest entry through the same path a person's install uses (the installer is cached and hashed by Magnus, and must be a file the proxy carried from the named source). It is class `reversible_write` on app `computer`, so Auto Review checks it against the request as a boundary action. Removing the app from the computer's apps reverses it.
- `sandbox_exec` stays as it is for runs and checks (decisions/2026-11-08-agent-computer-rulings.md); its description now sends the agent to `computer_exec` to change its computer.

Checks: `computer::agent_tools::tests`. Not yet exercised end to end on a live computer with a real download.
