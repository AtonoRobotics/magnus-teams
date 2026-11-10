# The computer's page says when the computer woke

**Choice.** The head of a computer's page (its state chip, and the buttons
that state allows: Put to sleep, Pause now, Resume) is redrawn whenever the
screen's look finds the computer in a new state. `desktop/tests/screens.mjs`,
the browser script `desktop_screens` drives, no longer presses a Start button,
and drives the team workspace as the team chat it is.

**Reason.** Since spec rev 131 there is no Start button: opening a computer
wakes it. The page drew its head once, from the state it had when opened, so
a computer opened asleep went on saying "Asleep", with no Put to sleep or
Pause now, after it was running, until the page was reloaded. The invariant
test `desktop_screens::the_desktop_screens_work_on_a_real_computer` timed out
at "make and start a computer" for both reasons: the Start button it pressed
is gone, and the page never said Running. Its script lives in `desktop/tests`
(not under `tests/invariants/`), and it also still drove the team screens
replaced on 2026-11-09 (channel and member forms, a Post to the channel
button). It now creates a workspace, sends, whispers and shares, invites by
email, and keeps a file and a knowledge entry in a project, as the team chat
does them.

`desktop/tests/computer-panel-check.mjs` checks that the page's head says
Running, with Put to sleep, once a computer opened asleep is awake; it failed
before the change.
