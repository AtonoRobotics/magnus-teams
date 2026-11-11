# Retention is set in Admin › Security, within the purge window

Spec rev 141, Administration › Security: "how long chats, files and logs are
kept". Security showed the trash and backup windows from the config and could
not change them (`decisions/2026-11-11-admin-sections.md` left that out).

Choice: an owner or admin sets two windows in Security:

- how long a deleted chat, file or memory waits in the trash, restorable,
  before its content is removed;
- how long each backup is kept.

Each window is a whole number of days from `RETENTION_DAYS_MIN` (1) to
`TRASH_DAYS_MAX` / `BACKUP_DAYS_MAX`, both equal to `PURGE_WINDOW_DAYS` (30)
(`crates/magnus/src/admin_app/retention.rs`). The reasons:

- The purge window is what Magnus tells people about deletion (spec, Files;
  the GDPR's month to act on an erasure): once an item leaves the trash, its
  content leaves the backups within 30 days. A backup kept longer than 30
  days would hold content deleted more than 30 days before, so the backup
  window may not exceed it. A trash window longer than 30 days would put off
  a deletion the person asked for beyond the month, so it may not either.
- At least a day, so a deletion made by mistake can still be restored and a
  backup is kept until the next is made (the config already refused 0 for
  backups).

What is not settable, and why:

- Live chats and files are kept until their person deletes them. Removing a
  person's live work after a time is a new promise about their data, which
  the spec does not make; it waits for the owner.
- The event log is the permanent record and is never edited (invariant); a
  person's content in it is removed when the content is deleted, which these
  windows time.

Enforcement: the sweeps that already run read the admin's setting each
round: the trash's hourly purge and the backup round's pruning (both read
`Kernel::trash_keep_days` / `backup_keep_days`, the setting or else the
config's). Setting a shorter window sweeps at once. The setting is a new
variant (`Retention`) of the existing `admin_setting` event, so no event kind
changes; it takes the admin step-up and is written to the admin audit
(`retention_set`, the windows before and after) before it is made.
