# #428 embedded auth spec (2026-11-07)

- Spec: `docs/proposals/embedded-auth.md` — local users, passwords+TOTP, invite links, sessions, console on the local store; cutover and the full removal list go to #429.
- Killed the Keycloak direction: #290, #292 and #356 closed as superseded (branch kept); #429 filed for the phase-1 build. The live Keycloak is untouched (verified answering on 127.0.0.1:19446 and identity.pyrallon.ai) and keeps serving sign-in until the cutover.
- Dead paths removed: the retired defaults in `config/keycloak-client.py` (admin args required), the dead setup-script references in `release/admin-on-appliance.sh`.
- Tests: new `the_client_script_requires_its_admin_origin_file_and_ca` (fails before: exit 1 on the dead CA); the `admin_on_appliance` file green.
