# Invite and status hardening (2026-11-07)

- Session resolution enforces status (#448): `Accounts::session` answers only for active people. Reason: the server never reloads the session file, so the CLI's `end_all` was file-only and live sessions survived a disabling; status reads fresh, so revocation now propagates cross-process without reloading anything.
- Accepting refuses a disabled person without consuming the token (#449). Reason: an outstanding invitation must not undo an administrative switch — only an explicit enable restores; the kept token stays usable afterwards.
- Invite and accept are `Accounts` methods under one mutex (#450). Reason: concurrent accepts of one token must admit exactly one, and concurrent invites must not resurrect a consumed removal; in-process only, matching every lock here (the CLI stays the rare second writer, as with the passwords file).
