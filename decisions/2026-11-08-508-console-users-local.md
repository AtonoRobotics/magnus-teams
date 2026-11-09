# Console user management on the local store (2026-11-08, #508 slice B)

Slice B of the #429 Keycloak retirement, after #507 (slice A: console
sign-in local-only). The Users panel and Sign-in rules stop reaching the
realm: the kernel owns people and policy, the console only renders them.

- Users list/detail/role/disable/enable/delete/cancel-invite are kernel
  endpoints (`/v1/admin/people*`), already shaped in `control.rs`.
  Reason: the kernel owns the one `Accounts` — roles, statuses, sessions
  and the last-admin guard stay in one advisory-locked place instead of
  split-braining across two processes.
- The last-admin race (#192) is decided by the kernel's refused
  `last_admin`, not a console pre-check under `admin_removals`.
  Reason: the guard must hold across processes (console plus owner CLI),
  so the database lock is the only check that cannot race; the console
  maps the refusal to its 409.
- An invitation makes a local person and a single-use token (7 days, the
  existing `Accounts::invite`), and the token is shown in the console
  for the admin to pass on — the appliance cannot send mail, so there is
  no email step (embedded-auth spec: "shown in the console (no email
  dependency)", the #352 pattern). The id derives from the email's local
  part, lowercased into the id alphabet; a held id refuses `taken`.
  Reason: the invite form has no username field, and the email already
  names them; refusing beats surprising (`dee2`) for an identity.
- Accepting is the existing `POST /v1/invites/accept`, verified through
  the API; the desktop invite screen lands with the desktop sign-in
  slice. Resend mints a fresh token and shows it again; the old one dies
  with it, as `invite` already replaces.
- The one-time password stays, for people without email (spec): a
  temporary password that never opens a session — `POST /v1/sign-in`
  answers `must_choose`, and `POST /v1/password` (temp plus the new one)
  sets the permanent password and activates. It keeps the invited
  status, unlike a set password. Reason: there is no provider page to
  force the change on, so the temporary credential can only choose, and
  a temp that signed in would be a second permanent password.
- Sign-in rules move to a kernel policy row (`account_sign_in_policy`):
  code optional/required, members idle/max lifetimes, lockout
  (enabled/failures/first wait/longest wait/permanent). The console
  reads and writes it; the kernel enforces it on the local path only —
  the provider sign-in keeps the provider's own rules until its slice.
  Reason: the rules the panel shows must be rules that bite, and only
  the kernel sees every local attempt.
- Code required refuses an unenrolled person, uncounted, answering as a
  wrong password. Reason: like a wrong password, it says nothing about
  the account; enrollment until the app enrolls is the owner CLI's
  `totp-enroll`, as the earlier slices set.
- Lockout extends while failures continue: each failure past the
  threshold holds until now plus the first wait, capped at the first
  failure plus the longest wait; permanent holds until an admin's
  enable clears the counters. Any status switch clears them.
  Reason: the form's four knobs must each do what they say — a longest
  wait that never caps, or a permanent lock with no way back, would be
  decoration.
- Member sessions take the policy's lifetimes at opening, and idle is
  enforced from a `seen` stamp refreshed at most once a minute per
  session. Reason: per-request exactness would write the store on every
  read; a minute's grace keeps the rule honest without it.
- `directory.rs`, `session::Idp`, the `private_key_jwt` proving use, the
  issuer requirement and the console's provider config (`issuer`,
  `client_id`, `idp_internal`, `idp_admin_url/ca`, `service_client_id`)
  are deleted. The stored key and `magnus-admin jwk` stay until the
  removal slice: `release/admin-on-appliance.sh` still registers the
  frozen live realm's console client from it, and that script's test
  pins the behavior. Nothing proves with the key anymore. Reason: slice
  B ends the console's conversation with the realm; deleting the realm
  registration itself belongs to the cutover that deletes the realm.
- `admin_admin_origin.rs` goes with the admin API it probes; the eight
  remaining console suites drop their Keycloak fixtures. The Keycloak
  harness, CI services and `sign_in.oidc` stay for the kernel provider
  sign-in and its tests (later slices).
