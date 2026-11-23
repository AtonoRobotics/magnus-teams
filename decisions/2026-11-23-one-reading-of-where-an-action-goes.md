# One reading of where an action goes; a rule on a site holds everywhere

**Choice.** `Kernel::destinations_of` (with `asks::destinations`, its pure
part) is the one reading of where an action goes. Cards, approvals, the
person's rules (`stopping_rule`, `covering_rule`) and the envelope all take
it. `rules::destinations_of` is gone. A web or browser call goes to its
address's host, a new source to its host, anything else to the recipients
the gate read, or, where it read none, to the addresses and channel
(`channel`, `channel_id`) the call names. Each is named as a rule names it:
a rule typed as a web address (`https://evil.example/`) matches by its host.

A person's own block or ask-first rule on a site takes in the sites below
it (`evil.example` stops `www.evil.example`), because widening a block or an
ask only makes the agent do less (2026-11-09). It holds wherever the action
runs, an `OpenWeb` research worker included.

An ask-first rule on a site asks the person in a run that can put a card to
them. An `OpenWeb` worker puts no card to anyone (2026-11-22), so there the
rule stops the action as a block does, and the model is told that the
person's rule stopped it and to carry on without it.

The site-allow remnants are removed: `asks::remembered` (unused outside its
test), the card's `allow_covers: "site"`, and the app's "Allow <site>" and
"Allowed for this site from now on". The scratch test that expected a
webpage card now expects none.

**Reason.** Rules read only email-style fields, so a person's "block
evil.example" on the web never fired, nor a rule on a chat app's channel
when the app declares no destinations. No webpage approval card or taint
URL gate comes back: only the person's own explicit rule, which they wrote
to make the agent do less. Asking in a worker is impossible, and letting the
action through would ignore their rule, so stopping it is the safer reading.
Third review, 2026-11-23, item 3.
