# Administration moves into the app: the guard, the step-up and the kill switch

Spec rev 141 (Administration) puts administration in the app the team already uses, on desktop and in the browser, and retires the VPN-only console built for the retired appliance. This is the foundation the Admin sections are built on. The console keeps running until another change retires it.

## Who is an admin

A signed-in person is an admin when the config gives them the `owner` role, when the config gives them an `admin` role, or when the server's own directory records them as an admin (`accounts::Role::Admin`, which the console's People screen sets). All three already mean "may administer": the owner always could, and the directory role is the one the console checks. Reading only one of them would lock out people who administer today. The app asks `GET /v1/admin/standing` to decide whether to show the Admin entry. Every signed-in person may ask, and the answer says nothing about anyone else.

## Step-up

Any admin request other than GET, HEAD or OPTIONS needs the session to have confirmed the admin's password, and their code if they use one, within `ADMIN_STEP_UP_SECONDS` = 300. Otherwise it answers `403 {"error":"step_up"}`. `POST /v1/admin/step-up {password, code?}` checks them exactly as sign-in does: through the identity provider for a person it keeps, otherwise against the server's own store. A wrong attempt counts on the same lockout counter as sign-in from the same place, so step-up gives no extra guesses.

- A step-up belongs to the session, not to the person. Another device of the same admin still asks. It is keyed by the SHA-256 of the session's credential, so the token is never stored.
- It is held in process memory and shared by the plain and TLS listeners. A restart forgets it, and the only effect is that the admin is asked again. Writing it to the log would put a security timer into the permanent record for no benefit.
- The console's own step-up stays as it is until the console is retired.

## The kill switch

Pressing any kill switch needs no step-up, because stopping is always safe. Releasing one needs a step-up. The guard exempts exactly the paths under `/v1/admin/kill-switch` that end in `/press`. The person, agent and connector levels will be mounted as `/v1/admin/kill-switch/<level>/<id>/press` and `/release` and need no change to the guard.

The server level is the switch that already exists (`Kernel::kill_switch`). It is reached through the console's control route `/v1/kill-switch` and writes the console's audit entries: `kill_switch_engaged` after pressing, so pressing works even when the log cannot take the audit entry, and `kill_switch_released` before releasing. `GET /v1/stops` lists, for any signed-in person, the switches that hold their work stopped, with who pulled each one and when. The app shows that as a banner to everyone a switch affects, which for the server level is everyone, admins included. This replaces the conversation's old banner, which said only that the switch was on.

## Reuse, not copies

The control socket's router is now `control::routes()`, and the app calls those routes in-process (`admin_app::mount::control`), with `actor` set from the session as `admin:<id>` and never taken from the client. `admin:<id>` is the form the console's own actions take (its `sub` has been the person's id since #508), so the control routes and everything that reads their events treat a change made in the app the same as one made in the console. `mount::pass(action)` mounts a control route at the same path, with the console's audit pattern: the audit entry first, no change without one, and `<action>_failed` when the change fails. Each section mounts the console routes it needs instead of copying their handlers.
