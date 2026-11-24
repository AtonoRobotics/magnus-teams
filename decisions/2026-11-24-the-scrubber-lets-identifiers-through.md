# The scrubber lets identifiers, slugs and paths through

**Choice.** `query_gate::scrub_check` no longer flags:
- a git object ID (40 hex characters, or 64 for SHA-256) in one case;
- a canonical UUID (8-4-4-4-12 hex);
- a lowercase slug with more than one hyphen, as before for URLs and now for
  any word (dated file names such as `2026-11-23-lane-project-knowledge-md`);
- a file path (a word starting `/`, `~/`, `./` or `../`), read one part at a
  time.

A credential prefix (`sk-`, `ghp_`, `AKIA` and the rest) counts only where a
word starts, so `task-12` and `risk-free` are not a key.

Still flagged: hex of any other length (a 32-character API key), mixed-case
hex, base64 keys (a `/` inside a word that is not a path keeps the word
whole), every prefix at a word start, and the canaries the invariants use.

**Reason.** The owner approved a narrower rule on 2026-11-24. The old one
flagged every commit ID and UUID: a teammate asking about a commit had the
lane refused, and an agent's report naming one waited in the owner's
Airlock. Narrowing by format, not by "mixed case only", keeps the hex keys
some services issue (32-character keys) flagged. A UUID-shaped key is the
one shape this lets through by design; the run's taint and the classifier
still stand behind the scrubber.
