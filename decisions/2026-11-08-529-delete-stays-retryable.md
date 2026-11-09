# Delete disables, erases, then removes (2026-11-08, #529)

A Codex finding on #528, verified before filing: removing the local
directory entry before erasing meant a failed erasure stranded the
person's remaining content and possibly connector credentials with no
retry — gone from the list, and the endpoint 404s.

- Deleting disables (guarded), erases, then removes the local entry.
  Reason: the disabling keeps the #527 property (a refused deletion
  erases nothing) and is idempotent, so a failed erasure leaves a
  listed, disabled account whose deletion retries cleanly — the
  erasure finds nothing new, and the removal follows. A disabled
  account is never the last enabled admin, so the trailing removal
  only fails the store itself.
