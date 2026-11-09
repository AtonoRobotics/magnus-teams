# #527 slice-B review hardening (2026-11-08)

Four Codex findings on #525, each verified against the code, each with a
test that fails without it (both unit failures and the e2e's 200-erased
observed before fixing):

- `admin_delete` removes/disables (guarded) before `erase_person`,
  propagates the config path's refusal instead of discarding it, signs
  out the sessions the removal ends, and answers their total. E2E:
  deleting the last admin returns 409 with account, sign-in and
  schedule untouched.
- `choose_password` and `set_password` delete the person's invites in
  the finishing transaction; `accept_at` clears `temporary`. Unit:
  a spent token resets nothing, and accepting after a temp signs in.
- `Accounts::invite_person` commits person, invited status, role and
  token under one lock; `admin_invite` calls it. Unit: the combined
  contract, and held ids refused.

Verified: lib 150/150; `admin_users` 5/5, `admin_people` 1/1,
`admin_security` 1/1. Full suite selection runs in CI.
