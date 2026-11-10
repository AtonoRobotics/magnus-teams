# Secrets typed into chat are masked before they are written

The spec (Credentials and vault, "Typed secrets") says a password typed into
chat is outside the vault's protection, and that the client steers the person
to the secure form and masks values in the transcript.

Choice: the kernel masks, not only the client. `typed_secrets::mask_event`
runs on every event as it is readied for the log, under the write lock
(`Kernel::number`, used by `emit` and `emit_all`). It masks the person-typed
text of `task_created` (chat turns, tasks, edits, team requests, whispers),
`schedule_set`, and a workspace channel message. The clear value is never
written, so no recalled conversation, export or model call can hold it. The
safer reading of "masks values in the transcript" is that no copy holds it,
since the transcript is what the model is sent.

What replaces a value is a marker: `[hidden password]`, `[hidden API key]`,
`[hidden access token]`, `[hidden private key]`, `[hidden PIN]`,
`[hidden secret]`, `[hidden card number ending 4242]`. The model reads what
was there without what it was. The app shows each marker as a masked value
(`Compose.maskedText`), and after a send whose reply lists `hidden`, tells the
person it was hidden and that a sign-in goes through the secure form.

Detection is narrow on purpose, so ordinary words, hashes and ids pass:
well-known key and token formats, private key blocks, card numbers with a card
network's prefix that pass Luhn, the password in `scheme://user:pass@host`,
bearer tokens, and a value given after a label ("password:", "pin=", "my API
key is X" when X does not read as a word). A value after ":" or "=" is always
masked: over-masking is the safe error.

Not done: moving a typed secret into the vault for the agent to use. The spec
does not promise it, and keeping a value the person typed would be a new
promise about their data. The secure form is the way to give Magnus a secret.
Event kinds are unchanged; events written before this stay as they were.
