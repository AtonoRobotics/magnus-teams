# Team Admin: who starts a workspace, owns it, and runs it

The owner, 2026-11-11: "Create Team Admin group. … Anything put in a team workspace is public to that team. The team should be isolated from other teams. Team Admin owns the workspace."

## The role

- **A server role a Magnus admin grants.** Admin › People › Workspaces lists the server's people with a Team Admin box (`PUT /v1/admin/people/{id}/team-admin`, `{ "on": bool }`), audited as `team_admin_set` after the step-up like any admin change. An identity provider that names a person's roles may name `team-admin` too.
- **Kept by the team ledger** (`Change::TeamAdmin`, `team::team_admin`), not beside owner/admin/member/guest in `RolesSet`, because a provider that names a person's roles replaces those, and would silently drop a role granted in the app.
- **Only a Team Admin starts a workspace** (`POST /v1/workspaces`; before, the server's owner or an admin), and the one who starts it owns it. The app offers the name field only to them (`/v1/admin/standing` carries `team_admin`), and tells everyone else "A Team Admin starts one and invites you."
- **Taking the role back** stops someone starting new workspaces; those they own stay theirs.

## Inside a workspace

Mapped onto the roles a workspace already had, not a second system: its Team Admins are its owner and anyone made an admin there. The app calls them "Team Admin (owner)" and "Team Admin". They invite and remove members, moderate and delete messages and files, set the team agent's channel limits, and (new: before, only the owner could) delete the workspace. The rules from `2026-11-11-workspace-delete-and-membership.md` stand: nobody changes a role above their own, and the last owner can be neither removed nor demoted.

## A Magnus admin and workspaces

A Magnus admin sees each workspace's name, its Team Admins and how many people and channels it has, and nothing in it. The admin routes that added or removed members (`POST /v1/admin/workspaces/{ws}/members`, `DELETE .../members/{person}`) and deleted a workspace (`DELETE /v1/admin/workspaces/{ws}`) are gone, and so is the app's path that used them: they let a server admin make themselves a member of any workspace and read its history, which the members were never told of. A server admin who is not a member gets 404 from every workspace route, delete included, as before.

What this leaves: a workspace whose only Team Admin leaves the server. Removing a person already transfers or deletes their projects; a workspace they solely own keeps running with its members and no one to administer it until that is built (open, below).

## Existing workspaces and people

Once, at the first start of this release (`Change::TeamAdminsSeeded`), everyone who owns a workspace becomes a Team Admin, and so does each of the server's owners, who could start workspaces until now and could grant themselves the role anyway; admins who are not owners do not, as starting teams is not support work. The seed is recorded, so a role a Magnus admin takes back later is not given again at the next start. Existing workspaces keep their owner, who is now their Team Admin.

## Spec sentences that now differ (spec rev 145)

- Workspace experience › Creating and joining: "A workspace, a project or a channel is created by typing its name." A workspace is created so by a Team Admin only.
- Team shared workspace › Roles: "Owner, admin, member and guest (read and chat only)." The owner and admins of a workspace are its Team Admins; the owner of a workspace is whoever started it.
- Team shared workspace › Membership changes: "Deleting a workspace deletes its volume, computers and channels within the purge window." Unchanged, but any of its Team Admins may, and no server admin.
- Administration › Spend: "Owners set spend limits per workspace and per channel." A workspace's Team Admin (its owner) sets its channels' limits; the Magnus admin keeps the workspace's total.
- "An admin grants a team connector … to the workspace with rules per role." Read as the workspace's Team Admin, which is what `POST /v1/workspaces/{ws}/connectors` already checks.

## Left open

A workspace left with no Team Admin, when its only one is removed from the server: the choice between handing it to another member and deleting it is the owner's (it trades a team's data against nobody running it).

## Tests

`admin_app::workspaces_tests::a_magnus_admin_makes_team_admins_and_stays_out_of_their_workspaces` (nobody but a Team Admin starts one, the server's owner was seeded, the grant needs the step-up and is audited, the admin list carries no channel, the admin routes into workspaces are gone, a workspace admin deletes it, a revoked Team Admin keeps their workspace), `team::team_admin::tests` (the seed, once), `desktop/tests/team-check.mjs` and `team-own-check.mjs` (the form follows `team_admin`, the role labels, no workspace controls in Admin).
