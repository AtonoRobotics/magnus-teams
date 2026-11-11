# The kill switch pauses the computer; a use waits for room; a computer from the first moment

Three gaps an audit found against the spec (rev 141 and 142), closed together.

**The kill switch reaches the agent's computer.** "Each stops new work and
running work at once, including the agent's computer actions." Until now a
switch refused the agent's next tool call, but whatever already ran inside
the computer went on. Now pressing a switch pauses the running computers its
work is on, the way the monitor pauses one (`pause_computer_as`, which
`pause_computer_from_monitor` now calls: every grant revoked and `Killed`
with `Pause`), and marks each with `HeldBySwitch { id, reason }`, a new kind
of computer change, in the same write. The server's switch reaches every
running computer. A person's reaches their own computer and those of the
runs it stopped (a team run they asked for is on the workspace's computer).
An agent's reaches the computers of the runs it stopped, and a person's agent
also its person's own. A connector's switch leaves the computers: a connector
runs in its own microVM, never in the agent's computer, so no work on a
computer is a connector's.

Releasing the switch resumes the computers it paused (`resume`, then
`Started`), so a program running inside goes on from where it stopped.
That is not the runs resuming: the runs still wait for their person's
Continue (K53); only the machine underneath is let go, as it was before the
press. A computer whose paused VM is gone by then (its host restarted) is
rebuilt on its next use instead, its files kept. While the switch is on, the
computer's person cannot resume it by hand. A computer paused for another
reason (its person's own pause, the monitor's) is not resumed by a release.

**A use waits for room.** "When it is asleep the agent's first use wakes
it." At capacity the tool used to answer "waiting for room ... Try again in a
minute." Now the call waits its turn in the queue, which the client already
shows with its position, and goes on once the queue starts the computer.
Only after `ROOM_WAIT_MINUTES` (15) does it fail, saying the server had no
room and the computer starts in its turn.

**A computer from the account's first moment.** "It exists from the moment
the person's account or the workspace does." Besides the look at start (which
makes the config people's computers then) and each minute after, the
computer is made in the background where accounts are made or first used: an
invitation sent (already), an invitation accepted, in the app or on the
invitation page, a sign-in at the identity provider, joining a workspace,
and `magnus person add`. The command line writes the directory from outside
the server, so it tells a running server over the control socket
(`POST /v1/people/{person}/computer`, which reads the directory again first)
and the server makes the computer at once; with the server stopped, its
start makes it. The minute's look stays as the backstop.

**Tests.** `computer/one_tests.rs`: the server's, a person's and an agent's
switch pause the computer, its person cannot resume it meanwhile, the release
resumes it, and a connector's switch leaves it; a tool call at capacity
waits, goes on once the queue starts the computer, and past its bound fails
saying so; a person added from outside the server has a computer as soon as
the server is told. `tests/scratch/tests/kill_switch_computer.rs` (Strix
Beta, real VMs): a program counting inside the computer stops counting while
each of the three switches is on and goes on after its release.
