# Team workspaces

**The agent acts for whoever asked.** A request in a channel starts a run whose person is the member who asked, so approvals go to them and their standing is what counts. Authority is the workspace's grants cut down to what that member's role allows. A guest can read and chat and cannot start a run.

**Channel text is data.** Only the requester's message is an instruction. Earlier messages come in beside it, labelled as data, and a request with other people's words in its history starts tainted as untrusted.

**A team run is cut off from the person.** No personal memory, relationship notes, playbook, personal files, personal apps, computer, browser identity or local computer. A built-in tool is allowed in a team run only if `in_team_run` names it, so a new tool stays out until someone decides otherwise. The tool list is cut where it is offered and checked again at the boundary. The memory pass does not run on a team run, so a thread cannot fill someone's personal memory.

**Apps.** An admin grants an app to the workspace for named roles. A member's own apps are not granted, so they never serve a thread, including the member's own request. A person removed from a workspace stops having any authority at the next action, and their running tasks are cancelled.

**The team volume.** Nothing enters except by an explicit upload or the agent's `team_write_file`. A file the agent writes after reading private data stays in the requester's space until they share it, which is how data fetched under one member's entitlements stays off the volume.

**Knowledge.** The team agent writes facts to the workspace's knowledge base directly and curators correct them afterwards, as the spec says. Facts are in every team run's context.

**Not here yet.** The team agent has no computer of its own: `computer_*` tools are refused in a team run, which is also why nothing fetched under one member's entitlements can land on a team computer. No sub-agents in a team thread, no whisper mode, no per-project knowledge bases, no working notes that expire with a task, no workspace-wide ceiling on local computers, and proactive team work is not built. Deletion is immediate, not on the purge window. The desktop app has no team screens.
