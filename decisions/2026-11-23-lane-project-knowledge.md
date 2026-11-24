# A person's agent asked in a project's channel has that project's knowledge

**Choice.** When a member's-agent lane runs for an ask in a channel tied to
a project, its prompt (`index_for`) carries that project's knowledge base
with the cite instruction, as the retired team-agent run had. It does so
only while both the person whose agent it is and the person who asked may
read the project (`Workspace::may_read`); out of the project, or in a
channel with no project, nothing of it is pushed in. An open-web lane never
gets it.

Entries from outside sources stay out, as before. Entries written by anyone
but the lane's person are shown only to a run already marked untrusted; a
new lane in such a channel is marked untrusted at dispatch, so it sees the
whole base. A run that is still clean is told how many entries were left
out and to read them with `team_read_knowledge`, which marks it.

**Why.** Since pure peer collaboration (4578c19) every ask runs as the
asker's personal lane, and the project knowledge lived only on the team-run
branch, so the agent in a project's channel lost it (invariant 8,
`a_projects_knowledge_stays_with_its_people_and_stays_correct`). Projects
are read only by their people, and the lane's output goes to the room the
asker chose, so both must be able to read it. Another person's words in the
prompt are untrusted by the rule `team_read_knowledge` already applies
(`owner::from_others`); putting them in the prompt without the mark would
let them through unmarked.
