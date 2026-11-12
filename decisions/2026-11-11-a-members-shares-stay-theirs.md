# What a member shares into a workspace stays theirs

The owner, 2026-11-11: "users should be isolated from each other securely unless they choose to shares something in teams. Then only what they share should be shared through teams. The user should still own it. The teams admin can moderate and delete it only from teams."

Spec rev 145 already says most of this. Shared files: "a file built from one member's entitled data goes to that member's space and reaches the team volume only when they share it. Members download and delete under their role, an owner or admin can do both for any file." Membership changes: "Their personal space is untouched." An audit found three ways the code broke it:

- **A held file was deleted with its member.** Files the team agent built from one member's data were kept in the workspace's own folder (`team/<ws>/held/<person>`). Removing the member, their leaving, or deleting the workspace deleted that folder, and the member's files with it. Now every file still held for someone is copied into their own library first (`Ev::FileUploaded`, their wall), and only then does the workspace's folder go.
- **Sharing a held file moved the only copy.** `share_held` renamed the file onto the team volume, so an admin deleting the team's copy, or the workspace, deleted the member's only one. Now the member's own copy goes to their library as it is shared, and the team volume gets the shared copy, which the workspace's owner and admins may delete as before.
- **Anyone could write over someone else's shared file.** A member's upload (`PUT /v1/workspaces/{ws}/files/{name}`), a share of a held file and the team agent's `team_write` all replaced a file of the same name on the volume, whoever had put it there, and the file then counted as the writer's. Deleting someone else's file takes an admin; replacing it took nothing. Now a write to a name someone else put there is refused (409), for an admin too, who moderates by deleting; the person who put it there still updates it.

Files a member uploads into a workspace were already copies of their bytes, so their library original is not touched by anything the workspace does.
