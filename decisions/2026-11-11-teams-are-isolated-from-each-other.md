# Teams are isolated from each other, and from people outside them

The owner, 2026-11-11: "Anything put in a team workspace is public to that team. The team should be isolated from other teams." And: "Then only what they share should be shared through teams. The user should still own it."

## The rule for someone in two teams

A team run reads only its own workspace: its tools are fixed to the workspace of its thread, whatever it is asked (already so; `team_read_knowledge` ignores a `workspace` argument in a team run). A person's own run may read a workspace's knowledge only while they are a member of it. Nothing carries one team's words into another team's run: a team run is given no personal context, and a member's own run that read a team's words is private, so it files and says them for that person alone.

## Holes closed (audit of every path, 2026-11-11)

- **A Magnus admin adding themselves to a workspace** to read it: the admin routes are gone (`2026-11-11-team-admin.md`).
- **A removed member still reading a project's knowledge** through their own run: a project kept its member and curator lists after the person left the workspace, and `Workspace::may_read` and `may_curate` checked only those. Now both need a current member, and removal takes the person off every project's lists (`team::knowledge::tests::a_removed_member_reads_no_project_of_the_workspace`).
- **A team's words leaving through a member's own runs**: a whisper's run carries its thread, and was labelled only untrusted (when others wrote in it), so it could file the team's words in knowledge open to the whole server. It is now always private. A team run's summary listed in a member's own conversation (`list_tasks`, `get_task`) now marks the conversation private too (`team::privacy_tests::a_teams_words_stay_private_in_a_members_own_runs`).
- **A workspace's computer looked up by a server admin outside it** (`POST /v1/computers` with `workspace`): now its Team Admins only. A Magnus admin still pauses or stops any computer from Admin (`computer::one_tests::everyone_gets_one_computer_on_start_and_the_next_start_makes_no_more`).
- **Other people's task words in the owner's conversation, and Spend's project and channel names**: `2026-11-11-a-magnus-admin-supports-and-reads-nothing.md`.

## Decided

- **Someone removed from a workspace keeps their own past requests to its team agent** in their task list: their words and what was answered to them, which they had already seen. They lose the workspace, its channels, files and knowledge.

## Left open

- The developer view and the run inspector (`dev.rs`, `anonymize.rs`) show each run as its person chose (`2026-11-11-anonymous-developer-data.md`): a tester's in full. A team run belongs to the member who asked, so a tester's request in a team shows that team's thread to a developer in full, though the rest of the team agreed to nothing. Deciding it (hide team runs from the developer view, or show them anonymous only) belongs to that work; left for it and the owner.
- The team chat's unsent drafts stay in the browser's storage after sign-out on a shared machine.
