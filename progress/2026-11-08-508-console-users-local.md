# #508 console user management on the local store (2026-11-08)

Slice B of the #429 Keycloak retirement, after #507. The Users panel and
Sign-in rules no longer reach the realm: the kernel owns people and the
sign-in rules, the console renders them.

Kernel (`crates/magnus/src`):
- `accounts.rs`: `account_sign_in_policy` row (code required, member
  idle/max lifetimes, lockout enabled/failures/first wait/longest
  wait/permanent) seeded from the config at open; failures extend the
  lock while they continue, capped at the longest wait, permanent until
  an enable; any status switch clears the counters. `temporary` on
  passwords: a temp never signs in (`MustChoose`), `choose_password`
  sets the permanent one and activates. Sessions take the rules' longest
  at opening, end when idle past the rules' idle minutes (`seen`,
  refreshed at most once a minute).
- `api.rs`: `POST /v1/password` chooses with a temp; sign-in answers
  `must_choose` for one.
- `control.rs`: `POST /v1/admin/people` (id from the email's local part,
  `taken` on a held id), resend, one-time-password store, `GET/PUT
  /v1/admin/sign-in-policy` (partial, ranges checked, longest >= first);
  the last-admin guard refuses `last_admin` under the accounts lock, and
  `turn_off` is folded into the disable endpoint.

Console: `users.rs`/`signin.rs` rewritten onto the kernel endpoints (the
invite token is shown once for the admin to pass on — the appliance
sends no mail; typed confirmation is the id when they have no email);
deleted `directory.rs`, `session::Idp`, the `private_key_jwt` proving
use, the issuer requirement and the console's provider config. The
stored key and `magnus-admin jwk` stay for the frozen live realm's
registration until the removal slice (`release/admin-on-appliance.sh`
still registers from it). Frontend shows the invite token, updated copy;
`docs/admin-console.md` claims re-pointed at the local store.

Tests: 5 new accounts unit tests (rules seed/round-trip, required code,
temp-only-chooses, lockout extend/cap/permanent/off, lifetimes/idle);
`admin_users.rs` rewritten local (invite/accept/resend/cancel, temp,
last-admin, disable/sessions/schedules, #192 race); `admin_security.rs`
rules land and bite (required code, lifetimes on new sessions, lockout
extension/cap/permanent with real waits); certificate, connectors,
knowledge, operations, people and shell off their Keycloak fixtures;
`admin_admin_origin.rs` deleted with the API it probed.

Verified: lib 147/147; the eight converted suites green
(users 4/4, security 1/1, certificate/connectors/knowledge/operations/
people/shell); audit/reach/local-sign-in/sign-in/on-appliance green;
backup 4/4, local_people, people 5/5, cold_start_proposals 5/5,
kill_switch, data_access, ended_schedules, all eight invariants
(52 tests) green. Full CI runs on the pull request (it touches
`desktop/`, so the installers and installed-app tests run too).
