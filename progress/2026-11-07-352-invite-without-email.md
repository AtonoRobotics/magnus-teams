# #352: invitations work without email

## What changed

- `crates/magnus/src/admin/users.rs`: `one_time_password`, which gives an invited person a temporary password and returns it, together with the provider's account page and their username.
- `crates/magnus/src/admin/directory.rs`: `set_temporary_password`; `person` now includes `username`.
- `crates/magnus/src/admin/mod.rs`: route `/api/v1/users/{id}/invite/password`.
- `desktop/admin/admin.js`: a "One-time password" button on each invitation, and a sheet that shows the password once. The invite toast now mentions the button when the email fails.
- `tests/scratch/sign-in-browser.mjs`: waits for the provider's form, because the account page reaches it by script. `keycloak.rs`: `browser_with`.

## Verification

- New test `without_email_an_admin_gives_an_invited_person_a_one_time_password` passes against a real Keycloak with no mail server. It cannot pass without the endpoint.
- `admin_users` 4/4 and the `magnus` lib's `admin::` tests 19/19 pass locally.
