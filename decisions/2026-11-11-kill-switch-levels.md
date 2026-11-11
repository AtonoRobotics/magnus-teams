# The kill switch at four levels, and Admin › Agents

Spec rev 141 puts the kill switch in the admin's top bar at four levels (everything on the server, one person's agents, one agent, one connector) and adds Admin › Agents. The server level is the switch that already existed; the three below it are new, in `crates/magnus/src/halts/`, mounted in the app through the admin foundation (`admin_app::agents`).

## Events

The levels below the server are one new event kind, `halt`, with changes `engaged` and `released` (a scope: `{level, id}`, and the name of who pulled it) and `paused` and `unpaused` (an agent). The log's actor and time are who and when. `kill_switch` is unchanged: the server level keeps its own event, its sealing of the vault and its revocation of sign-ins, and `magnus kill` on the server still moves it when the app or sign-in is broken. A level below the server seals nothing and revokes nothing: it stops work, and only the server's switch is the response to a compromise.

## What an agent is

The team agent is one agent (`team`), whatever workspace it works in: the spec names "the team agent". Each person has one agent (`person:<id>`). A run is the team agent's when it serves a team thread (its sub-agents do too, as they inherit the thread), and its person's otherwise, including that person's schedules, sub-agents and Magnus's background work for them.

A person's switch ("one person's agents") covers everything done for that person: their own agent and the team agent's work they asked for. Stopping is always safe, so the wider reading is the safer one.

## What each stops

Each level stops new work and running work in its scope at once, the way the server's does:

- New work is refused where it is created (`Kernel::create_run`), so a task, a chat message, a schedule's firing, a team request, a team routine, a sub-agent, a review and an opener are all refused. Schedules and routines under a held person or agent are not fired at all, so they do not count a failure.
- Every running run in scope is suspended at once with the reason `kill_switch:<level>:<id>`, its browser pages closed and its sandbox stopped; a person's or a person's agent's switch also stops that person's brokered connector microVMs. The run loop and the boundary check the switch at every step and before every tool call, the agent's computer actions among them, so a run that slips past the first sweep stops at its next step.
- A connector's switch takes the connector's tools out of every run's tool list at once, refuses any call to it at the boundary (suspending the run that tried), stops its microVMs, and suspends each running run that has called it, which is the work that needs it. It does not refuse new work that may not need it.

Releasing works as the server's does: it stops nothing more and resumes nothing by itself (K53). Each person is told, in the same write as the release, which of their tasks it stopped, and continues each from that notice or the task's Resume. A run stays held while any switch that covers it is still on. Background work that a switch stopped ends instead, as it does under the server's switch.

## Pause and stop

Admin › Agents offers Pause and Stop for each agent, and Stop for a person's agents together. Pause holds new work and lets running work finish. Stop is that agent's kill switch and stops running work too. The spec names both and the kill switch defines Stop, so Pause is the softer of the two. Pressing Stop needs no step-up because it is a kill switch press. Pausing and resuming are changes like any other and ask for a step-up, as the admin foundation's guard does for every change that is not a kill switch press.

## What the list shows, and what an agent did

The list shows, for each agent, what it runs now (tasks, chat turns, scheduled and background runs, each with its sub-agents), its schedules (a person's, or the team's routines), who asked, and what it spent. It shows no task's words, so it is not a read of anyone's content.

What an agent did is read from the log: each of its runs, newest first, with every boundary decision (the tool, the outcome and the stage), and no call's input. For a person's agent this is the person's data, so the read follows the person-data rule through the same mechanism as a read of their tasks (`Kernel::record_data_read`): it is refused where policy keeps people's data private, recorded in the log, and the person is told. The team agent's activity is the team's work, done in threads its members see, and is not a person's data.

## The banner

`GET /v1/stops` lists each switch below the server that reaches the signed-in person: their own person switch, their agent's, the team agent's when they are in a workspace, and every connector's, each with who pulled it and when. The app's banner shows them to the people affected and to the admins.
