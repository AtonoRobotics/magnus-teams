# Slice-B review hardening (2026-11-08, #527)

Four Codex review findings on #525, all verified against the code before
filing, fixed in their own pull request from master (the repo's rule for
review findings).

- Deleting removes (or disables) the account before erasing its data,
  and a refusal is never discarded. Reason: the erasure is
  irreversible, so the last-admin guard must run before it — a refused
  deletion leaves everything intact, and the config path (whose refusal
  was silently dropped, erasing and reporting success) refuses like the
  local one. Sessions the removal ends are signed out on the record, as
  a disable signs its own, and the answer counts every session the
  deletion ended, from both halves.
- Finishing an invitation spends its token: choosing with a temporary
  password, or a set password completing the invitation, deletes the
  outstanding invites in the same transaction. Reason: `accept` sets
  whatever password it is given for any non-disabled account, so a live
  token after onboarding is a password reset for whoever holds it.
- Accepting installs a permanent password: the upsert clears
  `temporary`. Reason: a temp set before the token was used left the
  account active but choosing forever, with no path back except the
  owner's shell.
- An invited admin is made in one accounts transaction (person, invited
  status, role, token). Reason: the role committed before the invited
  status left a window where the unfinished invitation counted as an
  enabled admin, through which a concurrent demotion of the sole admin
  could strand zero enabled admins.
