# Second review round: what changed and why

A review of the team missions, the tool gate and the remaining routes found holes. Each fix below is the safer reading of the spec, with a test.

- **A lane shares only a finished run.** A stopped or failed lane run fails the lane and releases nothing. Before, a cancelled run auto-released whatever it had.
- **Mission words are content.** The goal, objectives, drafts, contributions and deliverable move to the content store, owned by the requester, the lane owner or the releaser. Erasing a person, or deleting the workspace, deletes them. A deleted workspace's missions, standing releases and memory leave the ledger, and its working lanes stop.
- **Someone else's words are never the person's request.** A task made by another person, or by a run of another person (a teammate's mission lane, the team agent's delegation), is marked `by_other`. Its text names no destination of the person's and is not shown to Auto Review as what they asked (`Kernel::own_asked`). The verifier and the memory pass still read it as the task. A steer of a lane someone else asked for runs untrusted.
- **A mission has limits.** At most 8 lanes, a goal of at most 2,000 characters and a scope of at most 200. Another member's lane goes only to a non-guest who is in the room, on the model their own settings pick. A lane is released once (one lock across the check and the event), and the ledger ignores a second release.
- **Only a call puts an answer in the room.** An `@Magnus` mention, an ask or a direct chat posts the sender's agent's answer. A message that only names Magnus, or a helpful cue, holds the answer in the sender's Airlock. A shared selection labels answers "Answer:", so posting it starts no agent.
- **Peers add, admins lower.** A member may add people and keep roles, but only an admin lowers or removes someone else.
- **A server guest's invitation makes a guest.** An address new to the server, invited by a server guest, gets a guest account.
- **The computer's output is untrusted.** `computer_exec` and the desktop tools reach registries, app hosts and opened pages, so their results taint the run like a page. They are still not reviewed (spec: everything inside the computer passes without review).
- **An install is a boundary action.** `computer_install_app` adds the app's hosts to the computer's egress, so Auto Review checks it, as `decisions/2026-11-12-the-agent-works-on-its-own-computer.md` already said.
- **A link-following worker follows links.** An OpenWeb worker started from a tainted run opens only addresses from the person's words or what the web returned. The provenance check now applies to it, though it is never private itself.
- **Audits keep no secrets.** A passed-through admin change records secret fields (password, token, secret, credential and similar) as "(set)".
- **A live stream ends.** `/v1/tasks/{id}/live` stops when the client goes, the run ends or the session ends.
- **Connector answers are bounded.** One line of a brokered connector's answer is at most 32 MiB.
