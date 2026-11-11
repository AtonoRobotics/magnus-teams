# The owner sets the mailbox password in Settings, not in chat or the config

Recovery email needs the sending mailbox's password in the vault. The
teams server runs no admin console, and the only other ways in were the
command line on the server or pasting the password somewhere it would be
kept (chat, the config, a file).

Settings → Account now has an Email card for the owner only:
- `GET /v1/mail` (owner) says what `[mail]` holds and whether its password
  is in the vault, never the password;
- the password field writes through the existing owner-only
  `PUT /v1/secrets/{name}`, under the name `[mail] credential` gives;
- `POST /v1/mail/test` (owner) sends a test email now and reports the mail
  server's refusal, so a wrong password shows up here and not as a silent
  lost recovery.

Where mail goes (host, port, sender) stays in the typed config: it is set
once on the server and validated at startup.
