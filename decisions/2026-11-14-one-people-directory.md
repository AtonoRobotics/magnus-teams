# One people directory and one answer to "is this an admin": `people`

**Choice.** Everyone the server keeps is read through `k.people()`: `get(id)`, `exists(id)`, `ids()`, `all()`, `named()`, `name(id)`, `owners()` and `admins()`. `Kernel::person` is `people().get`. The module hides the merge: the config's `[[people]]` first, the server's own directory after, the config winning a namesake; then the log's overlays, roles set here and after them the identity provider's groups and roles; then the testers group. The server's own directory sits behind a small trait (`people::Local`), so the merge rule is tested as a table without Postgres.

What a person may administer is one answer, `k.authority(&person).await -> Authority { Guest, Member, Admin, Owner }`, the four roles the People screen already shows. An owner is an owner. An admin is one with the `admin` role here (config, `RolesSet` or the provider) or an admin in the server's own directory (`accounts::Role::Admin`). `Authority::administers()` is "owner or admin". It is read fresh from Postgres each time, like the admin guard always read it, so a demotion holds at once. The name avoids `accounts::Role` and `team::Role`: a Team Admin administers a workspace and nothing on the server.

**Reason.** A person came from four places and the merge was written out by hand eight times; four copies listed the config's people only and dropped people the server added (the share list, the policy panel's people, `owners()`, the models screen's roles). "Admin" was kept in two places and only the admin guard read both, so a person made an admin on the People screen could open Admin but could not stop a task, a schedule or a computer, lift an admin's suspension, or be matched by a policy's `context.is_admin`.

**Fixed with it.**
- The share list (`GET /v1/directory`) offers the server's own people.
- Controls (`tasks_for`, `get_task`, `cancel_task`, the schedule controls, lifting an admin's suspension), stopping a computer and the control policy's `is_admin` read the authority. An owner who also holds the `admin` role still reads `is_admin` in the policy, as before.
- Model access counts anyone who administers as `admin` (it read only the directory before; an owner with no directory row now counts too, as the admin guard always counted them).
- The console's kill switch count (`tasks_halted`) takes runs a narrower switch holds (`kill_switch:<level>:<id>`), through `halts::halted_runs`.
- `owners()` lists the config's owners first, in its order, then the server's own, so "the first owner" stays the config's.

**Kept on purpose.**
- Releasing the server's kill switch tells only the runs its own reason holds (`halts::held_by_server`): a run a narrower switch holds stays held, and its own release tells it.
- `roles::is_guest` reads the person's roles alone, without the directory: the boundary asks it on every action without a wait. `set_server_role` keeps the two in step, and where they differ it errs toward a guest's limits.
- Nothing a server admin may do grows beyond what the admin guard already granted (`2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`). Every check now uses the guard's own definition.
- `may_use_password` stays two calls, one rule (`people::may_use_password`): the command line reads the config's provider, the server reads the provider in use now.
