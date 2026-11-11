# People change their password in Settings and recover a forgotten one from the sign-in screen

The owner asked for "a forgot password and change password under settings".

**Change password.** Settings → Account has a Password card: the current password, the new one, and the new one again, at least `[sign_in] min_password_length` characters. It posts to `POST /v1/account/password`; `GET /v1/account/password` tells the page whether the password is kept here and the minimum.

- The current password is checked as sign-in checks it, and a wrong one counts on the same counter sign-in uses from that place (the network's, or the server machine's own). Guessing here locks sign-in as guessing at sign-in does. A wrong current password answers 403, not 401, so the page does not read it as the session ending.
- A new password that is too short, the same as the current one, or not typed the same twice is refused before the current one is checked, so it costs no attempt.
- The new password replaces the old one, every other session of the person ends (this device stays signed in), and a recovery link mailed before dies. The change is recorded as a new event kind, `password_changed` (`how`: `settings`), with a `signed_out` for each session ended. The password is never in the log.
- With an identity provider, only people the server itself keeps change their password here. Everyone else is told their password is kept by the provider. The owner's backup password under a provider is set only on the server's own machine, as before.

**Forgot password.** The sign-in screen, in the app and in a browser, has "Forgot password?" under a password sign-in (not under a provider's, which recovers on its own page). It asks for the email address and calls the existing `POST /v1/password-reset/request`. The page says the same thing whatever happens: "If that address belongs to someone here, a link is on its way", and to ask the owner if no email comes. The app sends it over its pinned connection with no session (a new `forgot_password` command), as it sends a sign-in. Following the link sets a new password and ends every session, as before, and is now recorded as `password_changed` (`how`: `reset`).

**Where the link points.** `[appliance] public_url` (for example `https://teams.pyrallon.ai`), the server's public address added for push subscriptions and checked when the config loads, is now also where recovery links point. The link uses `[mail] link_base` if set, else `public_url`, else the pairing hostname, and never the request's own Host header (#541).

**The owner's fallback.** On the server's own machine, `magnus password --config <file> --person <id>` reads a new password from standard input and sets it. It now also ends every session of that person, as a recovery does. It needs a shell on the server, so it works when no email can be sent, for any person who signs in with a password here, including the owner. For the teams server: `ssh` to it, then `~/.magnus-teams/bin/magnus password --config ~/.magnus-teams/magnus.toml --person <id>` and type the new password on its one line.

**What sending needs.** Recovery email goes out only with `[mail]` set (host, port, `tls`, `from`, and `username` with `credential`, the name of a vault secret holding the SMTP password) and a trusted link base. Recovery finds a person by the email recorded with them when they were added on the server (`magnus person-add --email`, or the console). People listed only in the config file have no email there and recover with `magnus password`.

Reason: the owner's request. Each choice follows what is already decided: counted attempts and a uniform answer keep passwords and addresses from being guessed (#533, #541); ending the other sessions is what a password change is for when a device or password was lost; links are only ever built from configured addresses.
