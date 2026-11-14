# One answer to "may they": `people::rights` beside `Authority`

**Choice.** Every question the server asks of a person's roles is asked in `people`, in its own words. `people::rights(&p)` answers from the roles alone, at once: `is_server_owner`, `held_to_guest_limits`, `reads_as_admin(authority)`, `sees_developer_view`, `may_approve(risk)`, `may_endorse_knowledge`, `may_write_policy_layer(layer)`, `own_update_channel`, `decides_deletion_requests`, `decides_app_tools`, `moderates_shared_knowledge`, `manages_every_device`, `judges_experiments`, `manages_shared_apps`, `uses_every_model`, `may_change_owners`, `may_choose_identity_provider`, `holds_team_admin_role`. "Do they administer the server" also reads the server's own directory and stays `k.authority(&p).await.administers()` (`2026-11-14-one-people-directory.md`). The question set is what the call sites asked; each answers as the site did.

The config's `Person` is data: `has_role`, `is_owner` and `is_developer` are gone from it, and so is `Config::may_use_password`; the command line asks `people::may_use_password` with the config's provider, the server `Kernel::may_use_password` with the one in use now. `policy_layers::may_write` and `admin_app::roles::is_guest` moved into `people::Rights`. `accounts::Role` stays where it is data: the directory's storage, the command line, the control API and invitations that set it.

A table test (`people/rights_tests.rs`) runs every question over a table of people (roles here, the directory's role) without Postgres. A guard test (`people/guard_tests.rs`) fails, naming file and line, if `.is_owner()`, `.has_role(`, `.is_developer()` or a `fn is_owner`/`has_role`/`is_guest` appears outside `people/`.

**Reason.** Thirty sites decided from role strings by hand, several spelling the same rule twice (the approver rule in `asks` and `auto_review`), and the rules lived on the config struct, so a new check reached for `is_owner()` instead of asking what the owner's right is. One module, with the questions named for what they decide, is where the isolation model (`2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`) can be read and tested at once.

**Changed with it.** Who may do what is unchanged, with one exception, toward what `Authority` says:
- The People screen's audit of a role change (`role_changed`) recorded the role before from the person's roles alone, so an admin by the directory showed as a member before. It now reads `Kernel::authority`. Tested in `admin_app::sections_tests::roles_are_owner_admin_member_or_guest`.

**Kept on purpose.**
- Model access matches role names against its rules (`spend::access::allowed`, `"owner"` in a rule list) and the owner-role check on a proposed role list (`settings`): these read role lists as data, not a person.
- The owner-only API guard (`api::owner`) is the owner, not anyone who administers: an admin supports, and the server's own settings are the owner's.
