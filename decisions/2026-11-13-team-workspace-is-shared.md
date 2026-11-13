# The team workspace is the team's shared space: writing there asks no one

**Choice.** A team tool that changes something inside the workspace (`team_post`, `team_write_file`, `team_note`, `team_add_fact`, `team_fix_entry`, `team_create`) stays in the workspace, as a write to a person's own account stays in it (`reaches_self`). It is not a send, it is not reviewed as one, and the Rule of Two does not ask about it. The team prompt says the workspace is seen by the whole team and tells the agent to post, save and add knowledge there without asking, and never to hand a post back to the member. The live work card in a team thread no longer counts steps ("· step 1"); it says what the agent is doing, as a personal chat does.

**Unchanged.** A file built from data only the requester may see is still held for them until they share it (invariant 8, `team_write`).

**Reason.** The owner (2026-11-13): "when I message magnus team agent it asks me to post it to the thread instead of just posting it. Anything that happens in the team workspace is viewable by the team. If I need private agent work I would use the private agent." And: "Magnus team agent still lists step 1 before responding."
