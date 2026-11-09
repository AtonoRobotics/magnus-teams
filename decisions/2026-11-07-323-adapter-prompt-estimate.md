# Adapter-side prompt estimate (#323)

The budget gate (#273) priced the kernel's bytes, but a chat adapter drops
attached documents it cannot send, replacing them with a placeholder line.
A large attachment then stopped a task whose budget covered the real prompt:
about $0.80 phantom on a $0.50 budget, suspended with the model never asked.

The gate now asks the adapter to measure the prompt as it would send it: a
new `estimate_prompt` op on the adapter contract, answered from the same
request-body builder the turn sends, so the estimate can never drift from
what goes out. The chars-a-token divisor moved into the contract as shared.
An adapter that cannot measure falls back to the kernel's bytes, and a
quote that fails still falls back to the full allowance, as before.

No timeout on the estimate: it mirrors `quote`, and a hung adapter breaks
the turn it was about to send anyway.
