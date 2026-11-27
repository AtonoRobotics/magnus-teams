# Every personal run starts with the person's remembered facts

The owner, 2026-11-27, asked whether runs should start with the person's
facts loaded rather than only when the model searches: "Yes, load them".

Choice:

- A run's system context carries the person's current facts, newest first,
  up to `MAX_LOADED_MEMORY_BYTES` (4096), in `person_context`, beside their
  relationship notes. These are the facts kept when the run started, so the
  context is the same at every call and the prompt cache holds.
- Only clean facts are loaded, neither untrusted nor private. A fact filed
  by a run that had read outside or private content keeps that label, and
  loading it would give it to every run untainted. A line counts the facts
  left out (tainted, or past the budget), and `memory_search`, which carries
  the taint of what it returns, finds them.
- Team runs and a project's channel get none of it: personal memory never
  appears there (spec).

Reason: the spec has Magnus act "on details the user mentioned only once".
That cannot depend on the model deciding to search. Under the person-only
rules a person has a few dozen one-sentence facts, which is small enough to
carry in full.
