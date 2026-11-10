# The teams desktop app is "Magnus Teams", app.magnus.teams

The teams desktop app was built as "Magnus" with the identifier `app.magnus.desktop`, the same identity as magnus-claude's app. On 2026-11-10 the owner's app was magnus-claude's (0.7.1-test.1070, updating from magnus-claude's server and signed with its key). It never got Teams or Computers, and a teams update could not install over it.

The teams app is now "Magnus Teams" with the identifier `app.magnus.teams`. It installs beside magnus-claude's app, keeps its own settings folder (server, certificate pin, session) and updates only from the teams server with the teams key. Window titles stay "Magnus", the agent's name. Package names on the update service keep the `Magnus_<version>_…` form: the release copies Tauri's `Magnus Teams_…` output to them, so the manifest URLs have no space and publish.sh is unchanged.

Reason: the owner's rule that magnus-teams is its own app (own key, update service, server and certificates). With one identity, installing one app replaced the other and both shared one settings folder.

An install of the teams app before this change, which no one is known to use, keeps its old settings folder. The first launch of "Magnus Teams" signs in afresh.

Test: `tests/scratch/src/ci_scripts.rs` `the_desktop_app_is_not_magnus_claudes` (fast tier).
