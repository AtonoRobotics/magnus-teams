# #541 recovery review hardening (2026-11-08)

- `api.rs`: `link_base` returns `Option` (configured base or pairing hostname); the request handler warns and answers ok without a base, and spawns SMTP delivery; `cookie_guard` exempts POST /reset.
- `accounts.rs`: `remove_person` deletes `account_reset_tokens`; `confirm_reset_at` rejects tokens for people with no record and spends the orphan.
- `config.rs`: `Mail::link_base` docs say no base means no mail.
- Tests: `removing_a_person_spends_their_recovery` unit (failed before the fix: token survived removal; green after, with the round trip still green); e2e main flow follows the token locally with a stale cookie, plus `recovery_sends_nothing_without_a_trusted_base` and `recovery_answers_before_the_mail_server_does` — e2e runs in CI per user preference.
- Next: #535 (console invites email again via the mailer), then #534 (approval signup).
