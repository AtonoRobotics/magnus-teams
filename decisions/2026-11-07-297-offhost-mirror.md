# #297: mirror backups off host to copy_to

Each backup is mirrored off host after it is made and old ones are
dropped: `[backup] copy_to` names an rsync destination, a path or
`host:path`, from configuration only. Nothing configured, no off-host
copy — the same rule #288 states for the legacy bundle.

## Choices

- rsync with deletions, so a pruned backup leaves off host too. The copy
  runs inside `back_up`, after prune, bounded by `copy_timeout_secs`
  (default 300, also configured).
- A failed copy is recorded, not a failed backup: `BackupCopied`
  (file, ok, detail) joins the log, `last_copy` the projection, and the
  answer and status page say whether the copy held. The local backup
  stands either way.
- No destination blocklist: refusing `zero` in code would be another
  hardcoded parameter. The owner configures strix-beta; nothing is
  the default.
- Scrub rewrites are not mirrored at once; the next backup carries them.
