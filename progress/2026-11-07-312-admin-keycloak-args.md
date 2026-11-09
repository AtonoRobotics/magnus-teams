# #312 admin Keycloak args (2026-11-07)

- Root cause: master desktop's "admin console on the build box's appliance" step failed 5+ runs with `FileNotFoundError` at `keycloak-client.py:54`. `/etc/magnus-production-origin/ca.pem` exists nowhere in the repo and nothing creates it: the old production Keycloak was removed with the retired deployment (see #292's notes). The live origin serves a cert from "Magnus appliance identity CA", verified OK against `~/.magnus-appliance/identity-ca.pem` (`status_admin_ca`).
- Fix: `release/admin-on-appliance.sh` resolves the three values from the appliance config and passes them explicitly; clear errors when the identity is not configured. Verified the resolution against the live box config (url + CA + admin.json all present).
- Safety: #292 leaves the admin-console client shape unchanged, so the client's merge-update over the live `magnus-admin`/`magnus-admin-svc` is a no-op; role re-assignment is the only write, and any failure there is loud, not silent.
- Tests: new negative test fails before (exact production traceback) and passes after; full `admin_on_appliance` file 2/2. Positive proof is the master desktop run going green after merge.
