# A workspace's people are in reach from inside it

The owner, 2026-11-12: "team has no way to add or delete members from a team workspace."

## What was wrong

The server had everything (`POST /v1/workspaces/{ws}/members`, `DELETE .../members/{person}`, `GET .../people`, `POST .../invites`), and so did the app's people page (`team-people.js`): add someone on the server, invite by email or link, change a role, remove with a confirmation, leave, delete. But the only way to it was a small "People and invites" link at the very foot of the left rail, below every project, channel, direct message and the archived list, so in any real workspace it sat below the fold; the command menu was the other way. The right panel's Members card, where the spec puts members ("A right panel shows the open project's files, tasks, members, …"), showed initials and nothing to do, and a project's Members tab only adds workspace members to the project.

The people page also bound its invite form again every time it redrew after a change, so after one add, role change or removal, each invite was sent (and emailed) twice.

## Choice

- **The rail names the page second, under Activity**, above projects and channels: "People: add or remove" for the workspace's Team Admins (its owner and admins), "People" for everyone else, with how many are in.
- **The right panel's Members card links there**: "Add or remove" for a Team Admin, "See all" for anyone else, and each face carries the person's name (a tooltip and screen-reader text), not only initials.
- **A project's Members tab** adds, for a Team Admin, a link to add or remove people in the workspace beside the invite form.
- **The people page puts adding first** for a Team Admin: someone on the server by name with a role, someone new by email or by a copied link. Below it, everyone in the workspace; a member or guest sees that list with roles and is told a Team Admin adds and removes people, with no controls. Removal still asks first ("They lose it at once and their running work here stops. What they wrote stays.").
- **An invite form is bound once per element**, so a redraw does not send an invite twice.

Nothing changes on the server, and no rule of who may do what: the rules of `2026-11-11-team-admin.md` and `2026-11-11-workspace-delete-and-membership.md` stand.

## Removal, against the spec

"Removing a member revokes their grants, ends their running tasks and leaves team files in place. Their personal space is untouched." The kernel already does this (`Kernel::set_workspace_role` with `None`: the team agent's tasks for them are cancelled, files held back for them go to their own library, `Change::Removed` takes them out of the workspace and every project; the team agent's authority is the workspace's grants cut to the requester's role, so with no role there is none). No test pinned the running tasks or the files, so one does now.

## Tests

- `desktop/tests/team-members-check.mjs` (new, in the fast tier): the rail's way to People sits above the projects for owner, admin and member and says "add or remove" only to Team Admins; the panel's Members links there with names; a Team Admin's page offers add, email, link, role and remove, a member's and a guest's none; a cancelled removal sends nothing and a confirmed one sends the `DELETE`; an invite goes once after a redraw. It fails on the previous UI.
- `team::membership_tests::removing_a_member_ends_their_running_tasks_and_leaves_team_files`: a removed member's running team task is cancelled, the team file they added stays readable by the team and not by them, and they can no longer post to the workspace. It fails with the task-ending line taken out.
- `desktop/tests/screens.mjs` follows the link's new name.
