# Magnus forgets a fact the person corrects or takes back

The owner, 2026-11-27, asked whether the model should be able to forget or
correct a fact itself: "Yes, add it".

Choice:

- A new tool, `memory_forget {id}`. The id is shown in brackets with each
  fact: in the facts loaded into a run, in `memory_search` results and in
  the memory pass's "Already remembered" list. A correction is a forget
  followed by a `memory_write`, with no separate "replace" mechanism.
- It has the same bounds as a write: only in a run the person spoke in,
  only their own current facts, and under the same per-run cap, which now
  counts writes and forgets together.
- A forgotten fact goes to the person's trash, as Forget in the app sends
  it, so a wrong forget can be restored.
- A run that has read untrusted content cannot forget a fact from a clean
  run. This keeps the retired curator's anti-poisoning-by-deletion rule, so
  a page cannot erase what the person taught.
- A fact written in a run that is still going cannot be forgotten until
  that run ends. Deleting a memory also removes where its run wrote it
  (invariant 3). The model is told why.
- The memory pass may use `memory_forget` too: corrections are mostly
  heard there.
- Team runs never get the tool.

Reason: once facts are loaded into every run, a stale one keeps steering
Magnus until someone removes it. The person should not have to go to
Settings to fix what they already corrected in conversation.
