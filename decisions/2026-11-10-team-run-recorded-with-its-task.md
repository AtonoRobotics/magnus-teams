# A team run is recorded with its task, in one write

**Choice.** `create_run` takes the team thread a run serves (`NewRun::team`)
and writes it with the task's `TaskCreated` in one `emit_all`, before the run
is woken. A team request, a team routine and a sub-agent of a team run all
pass it there; none records it afterwards.

**Reason.** The team thread was recorded by a second event after
`create_run` had already woken the run. A first model call that went out
between the two found no team thread for its task and was built as the
requester's own: their relationship notes in the system context and their
memory tools offered. The invariant test
`inv8_team::personal_context_and_credentials_never_serve_a_team_thread` saw it
in about one run of three on Strix Beta ("personal context reached a team
thread: a relationship note", and once "memory_write was offered"). Invariant
8 (team) says nothing personal to a member serves a team thread, so there can
be no moment when a team task exists without its thread.

`tests/scratch/tests/team_personal_context.rs` makes sixteen team requests
and checks every call of them; it failed before this change and passes after
it.
