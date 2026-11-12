# A person's computer is theirs alone; an admin may only pause it

Spec rev 145, Multi-agent and multi-user boundaries: "Each user has their own VM, their own key in the one credential service, and their own approval grants. Nothing on one user's computer is reachable from another's." Agents: "An admin can pause or stop any one of them." Operator visibility: developers "see a screen or files only in a session the user grants". The owner, 2026-11-11: "users should be isolated from each other securely".

The computer routes' ownership rule (`may_control`) let the server's owner and any admin act on every person's computer, so an admin could read its activity, pages and stored-credential names (`GET /v1/computers/{id}`), add egress destinations, open pages, put a credential under a name the person's agent already uses (`surrogates`, which then carries the admin's account into the person's traffic), install and remove apps, back up, restore over newer files, reset and destroy it. None of that was logged as a read of the person's data or told to them. Now:

- A person's computer is changed, read and run only by that person (`may_control`).
- A server owner or admin may still pause it (`POST /v1/computers/{id}/kill` with `mode: pause`, `may_stop`), since stopping is always safe and the kill switches already stop it.
- A workspace's computer is controlled by the workspace's owners and admins, and opened (screen, files, takeover) by its members. A server owner or admin who is not a member is not one; they can add themselves through Admin, which is audited.
- An operator session the person granted reads files under `/workspace/` only, as the person's own file routes do, not the computer's home, browser profiles or cookies.
