# A browser check fails by name instead of hanging

On 2026-11-13, the scratch test `model_picker` hung on Strix Beta for over 90 minutes, on main and on the chat branch alike. It ran its browser check with `Command::output()`, which has no limit. The script had stopped at a step that has none either.

**The cause.** Changing the default model (`PUT /v1/model`) is an admin change held for the password again (step-up, `admin_app/settings_gate.rs`). The page's `api()` opened the "Confirm your password" sheet and waited for a person. The check was written before step-up, so nobody answered it. The check now answers the sheet as the admin would, when it opens.

**So a check can never hang a run again:**
- `magnus_scratch::browser_check` runs a check in a process group of its own, within `BROWSER_CHECK_LIMIT_SECS` (300). When the script exits, whatever it left in its group is killed, so a browser that outlives it cannot hold the output open. Past the limit, the whole group is killed and the test fails with what the script printed.
- The script marks each step and gives up after 240 seconds with `stuck at: <step>`, so a failure says where it stopped. That is how this cause was found.

The other scratch tests that run node still call `output()` directly. They move to `browser_check` as they are next touched.
