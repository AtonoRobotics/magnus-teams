# `magnus kill` on the server

Spec rev 141, Administration: "magnus kill on the server itself still stops everything when the app or sign-in is broken." The command left with the retired console's binary; it is back in `magnus` itself (`crates/magnus/src/kill_cli.rs`).

- `magnus kill --config <file>` presses the server's switch; `--release` releases it. Both go through the control socket (`<state_dir>/control.sock`, mode 0600), so they need no app, no network and no sign-in, and move the one switch the app moves (`Kernel::kill_switch`).
- The actor is `server:<user>`, the local user running it (from the process's uid and `/etc/passwd`), so the log and the banner name the server's operator ("root on the server"), never an app account.
- Each writes the same admin audit entry as the app (`kill_switch_engaged` / `kill_switch_released`, source `server`). Pressing moves the switch first and records after, because stopping waits on nothing; releasing records first, as the app does.
- Releasing from the server needs no step-up. Opening the socket already takes the server's own user or root, which is more than the password and code the app's step-up asks for. Without it, a broken sign-in would leave the server stopped with no way back but editing the log.
- When the server does not answer within `KILL_ANSWER_TIMEOUT_SECONDS`, the command says so and changes nothing. It does not write to the log itself: the running kernel checks that the log has no gaps, and an event written behind its back would stop it uncleanly. Stopping the service stops everything in that case.
