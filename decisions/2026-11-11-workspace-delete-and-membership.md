# Deleting a workspace, and who is in one

The owner asked: "teams and direct messages can't be deleted. The admin or the team owner should be able to delete the team", and "there is no way to add or remove users from teams."

## Deleting a workspace

The spec (rev 145, Team shared workspace › Membership changes): "Deleting a workspace deletes its volume, computers and channels within the purge window." The kernel already did that (`delete_workspace`: ends its work, deletes its computers, removes the volume, takes the workspace's team events' content out of the log and scrubs the backups), behind `DELETE /v1/workspaces/{ws}` for the workspace's owner. Nothing in the app called it, and a server admin who was not the workspace's owner could not reach it.

- **The workspace's owner deletes it from the workspace** (People and settings › Delete workspace). The app asks them to type the workspace's name first, as GitHub and Slack do for an irreversible delete; the server's check is the owner role. A workspace admin cannot delete it: the spec gives destructive control of the workspace to its owner, and an admin of the server.
- **A server admin deletes any workspace** from Admin › People › Workspaces, through `DELETE /v1/admin/workspaces/{ws}`. It is an admin change, so it sits behind the Admin guard: "Each admin change asks for the admin's password and code again if they have not confirmed them in the last 5 minutes" (Administration). It is audited first, like every admin change (`workspace_deleted`, with the workspace's name and members as `before`; a failure is `workspace_deleted_failed`). The log names the admin (`admin:<id>`) as the actor of the deletion's events, not a person.
- **Members lose it at once.** The ledger drops the workspace with `Change::Deleted`, so every workspace route answers 404 from that moment, and the app, on its next look, leaves the workspace and says it was deleted.

## Who is in a workspace

The API already added server people by email, set roles and removed members; the app had these only on a page at the bottom of the rail, and three rules were missing.

- **A workspace keeps an owner.** The spec (Administration › People): "the last owner cannot be removed." Applied to a workspace: its last owner can be neither removed nor demoted, by anyone, server admins included; ownership is handed on by making another member owner, after which the old owner may step down or leave. Before, a workspace admin could remove or demote the owner.
- **Nobody changes someone whose role is above their own**, as nobody gives a role above their own already. An admin manages members and guests and other admins, never an owner.
- **Anyone leaves on their own** (`DELETE /v1/workspaces/{ws}/members/{me}`), except the last owner. Before, leaving needed the admin role.
- **Adding someone the server knows is a pick, not an address.** `GET /v1/workspaces/{ws}/people` lists the server's people not yet in the workspace, by name and id only, never their email addresses, for the workspace's owners and admins; the people page offers them as a list to add with a role. Someone new to the server is still invited by email (`newcomer`) or link, unchanged.
- **A server admin manages any workspace's members** from Admin › People › Workspaces (`POST /v1/admin/workspaces/{ws}/members`, `DELETE .../members/{person}`), audited as `workspace_member_set` and `workspace_member_removed`, and under the same rules: a server admin acts as an owner would, so the workspace still keeps an owner.
- **Removal ends access at once** and, as the spec says ("Removing a member revokes their grants, ends their running tasks and leaves team files in place"), ends their running tasks there and drops files held back for them. What they wrote stays: the spec deletes a person's data when they are removed from the server, not from a workspace.

Admin › People holds the workspaces, rather than a new Admin section, because the spec's sections are fixed and People is where it puts roles and removal.
