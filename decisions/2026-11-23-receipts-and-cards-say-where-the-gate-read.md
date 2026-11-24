# Receipts and cards say where an action went as the gate read it

**Choice.** Before an action of the model's that changes something, the
gate records where it goes, as it reads it (`Kernel::destinations_of`, a
draft's recipients included), in a new evidence record,
`Change::Destinations { run, call_id, to }`. Its `to` is content, deletable
with the run's other content. A receipt names those destinations and
nothing else; evidence.rs's own reading (every address anywhere in the
model's input, the body included) is gone. A receipt for an action recorded
before this names no one rather than reading the input.

The approval card's preview is built only from what the gate read
(`details.destinations`), or where it read no recipients, from where the
server says the action goes (`ask.destinations`). It never reads the
model's input; the exact call is still one tap away. The card's "Spend
guardrail: Capped at $X · Vault-only credentials" line is removed: the
server sends no guardrail or mandate field for an ask, and nothing enforces
a cap taken from an amount, total or price in the model's input.

**Reason.** A body saying "cc'ing carol" put carol in the receipt, and a
`send_draft` by its id named no one, though the gate read the draft's
recipients. The card could show the model's words as the message being
sent, so the person allowed a different email from the one that went. A
cap line nobody enforces is a promise the server does not keep. Fourth
review, 2026-11-23, item 6.
