# The accounts rules stand apart from the store (2026-11-14)

`accounts.rs` held the sign-in rules, the lockout, the reset and invite
decisions and the session lifetimes inside the SQL that read their rows.
Every rule could be tested only against Postgres, and the password-length
check and the Argon2 hash were copied six times each.

- The lockout is a pure `accounts::lockout::Lockout` (locked, failed,
  succeeded, forgive, clear) that takes the instant and the sign-in rules.
  `Accounts` keeps thin wrappers that pass `Utc::now()` and the cached rules,
  so each call site, and its order of check and count, is unchanged (a
  locked attempt is still counted). Reason: the rule is tested at fixed
  instants.
- One `phc::long_enough` and one `phc::hash`, called where each copy stood:
  a short password is still refused before an invitation or recovery token
  is touched, with the same words.
- The reset request (Nothing, Silent, Issue), the reset confirm (Consume,
  DropToken, Refuse), the invite accept (a disabled invitee refuses without
  consuming) and a session's life (end, liveness, restamp) are pure functions
  in `accounts::rules`, called by the SQL code on rows it read inside the
  same transaction. There is no store trait for invites, resets or sessions.
  Reason: their correctness is Postgres's — the advisory lock and the
  `DELETE … RETURNING` that admits one racing use (`racing_accepts_admit_exactly_one`)
  — and a trait would invite an adapter without it.
- A session is judged on the store's clock: its row is read with `now()`, so
  expiry and idling use one clock as before. The bare `'60 seconds'` is
  `SESSION_RESTAMP_SECONDS`.
- The sign-in check reads through a narrow `Credentials` trait: password,
  status, totp_secret, advance_totp_step (one compare-and-set statement), and
  take_machine_code. The last is beyond the four first named: the owner's
  backup is part of the one check, and its machine code is the check's only
  other read. `Accounts` is the Postgres adapter; a `Table` in memory is the
  tests', so the check's rules run without a database. The Postgres tests
  stay beside them.
- The check no longer holds the store's connection across the Argon2 verify;
  each read takes it in turn. The rules are read once at the check's start
  (they were read at the start, and sometimes again at the backup's
  sign-in). Nothing a person sees changes.
- `SentSearch` and `Destination` (with their faults) moved from `config.rs`
  to `gate_reads.rs`, their only reader, as `read_back::ReadBack` lives with
  its reader. Config keeps the fields.
