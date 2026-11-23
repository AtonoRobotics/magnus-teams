# 2026-11-22: Retire the team-agent run path

## Decision
Nothing starts a team-agent run any more. The pure-peer decision
(`2026-11-22-pure-peer-collaboration-personal-agents.md`) moved `@Magnus`
onto a `MemberAgent` mission lane in the asker's personal agent. Two paths
still started the old team agent, and they move the same way:

1. **Routines** fire as a member lane of their author's (`r.by`) personal
   agent in the routine's channel. The lane is started by `poll`, so it is
   background work that asks no one, as a routine always was. Its scrubbed
   answer auto-releases into the channel, because the author set the routine
   up to report there. `RoutineRan` records the lane's task, so the sweep's
   one-open-run and stop-after-failures rules work as before. A routine's
   instructions are its lane's objective, so they are held to the
   objective's limit (2,000 characters, down from 4,000) and to
   `scrub_check` when a routine is added or changed, not only when it fires.
2. **Quiet-thread follow-ups** run as a member lane of the person whose
   question went quiet. The lane is started by `poll` and gets only that
   person's own question, never the thread. Its answer is held in their
   Release Airlock, as when Magnus is only named, because nobody called on
   it. A follow-up that cannot start is logged, and the rest still run.

`Kernel::start_room_mission_as(actor, …)` starts these lanes.
`start_personal_room_mission` is that function with the member as the actor.

With no caller left, these are deleted:
- `team_request`, `team_request_on`, `team_request_as` and `start_team_run`,
  with the start-time spend hold (`STARTING`, `TeamLimited`,
  `team_budget_left`, `committed_this_month`).
- `team/delegate.rs` and the `team_ask_personal_agent` tool. The tool checked
  only the target's role, not whether the target could see the channel, and
  sent 40 messages of channel history into the target's run.
  `panel_delegations`, which the side panel still reads, moved to
  `team/whisper.rs`.
- The team agent's own tools, which no other run could use: `team_read_file`,
  `team_write_file`, `team_post`, `team_add_fact`, `team_fix_entry`,
  `team_create` and `team_note`. `team_read_knowledge` stays, because a
  person's own agent reads a base with it.
- `crates/magnus/src/peer_agent.rs`, which nothing called. The `PeerAgent`
  dispatch arm also goes: it recorded a lane that nothing would ever run.
  Creating a mission now refuses a `peer_agent` lane before anything is
  recorded. `LaneKind::PeerAgent` stays, because stored `lane_dispatched`
  events can carry it.
- `dispatch_one_lane`, which nothing called.
- Routes the app never calls: `GET /v1/workspaces/{ws}/memory`
  (`search_workspace_memory`), `PUT /v1/profile/team-apps` and
  `GET /v1/workspaces/{ws}/routines`, with their unused `server.js` wrappers.
  An app a person hid with `AppVisibilitySet` in an older release stays
  hidden: its reader stays, which is the safer reading.

These stay, because the log holds team runs that older releases started and
one of them may resume: `TeamRun` and `Change::Run` as data, `refusal`,
`index_for`'s team branch, `RunKind::TeamRequest`/`TeamRoutine`, the
workspace's own sign-in for a team run, `stop_partway`, the month's team
spend and the `/limit` route (`team-command.js` calls it). Every event kind
keeps its reader. Tests of what still reads a recorded team run use
`recorded_team_run`, a test-only fixture that records one the way older
releases did.

## Reason
A second way to run shared work, with its own tools, spend and prompt,
contradicted the pure-peer decision. It also carried a real bug in
`team_ask_personal_agent`. Moving routines and follow-ups onto the asker's
own lane gives every shared request one path, one privacy boundary (the
Release Airlock) and one budget, the member's own. What older releases
recorded still replays.

## Invariant tests
`tests/invariants/tests/inv8_team.rs` drives the deleted team tools in
`working_notes_are_scratch_and_end_with_their_task`,
`a_sub_agent_in_a_team_thread_keeps_the_threads_limits` and the
file-and-fact part of `a_projects_knowledge_stays_with_its_people_and_stays_correct`.
The pure-peer decision already sent those asks to a personal lane, where
the team tools were refused. The patch that drops these tests is
`r3-retire-agent.invariants.patch`, for the owner to apply.
`tests/scratch/tests/team_personal_context.rs` is deleted. It guarded a race
in starting a team run that no longer exists, and asserted that an ask never
sees its asker's own context, the opposite of the pure-peer design.
