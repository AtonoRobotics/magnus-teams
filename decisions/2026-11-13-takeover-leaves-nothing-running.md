# A takeover leaves nothing running

A person who holds a computer's screen can start anything on it, as root,
and before this it kept running after "I'm done": a screen or clipboard
recorder, a keylogger, a loop that keeps acting. On a workspace's computer
that is one member watching the agent work on another member's request.
Invariant 18's limit said so: "It cannot stop a process that root left
running in the computer."

**The boundary.** Taking control of a computer nobody holds first has the
guest record a boundary (`takeover_begin`, `sandbox/guest/src/leftovers.rs`):
the desktop is started, so it is not the session's, then every process
running (pid, start time, executable) and every autostart entry is noted.
A second person taking over keeps the first one's boundary. If the guest
cannot record it, taking control is refused with the reason, before
anything is logged: a takeover whose leftovers could not be told apart could
only end with the agent paused.

**Handback stops what the session started.** `takeover_end` finds every
process not at the boundary, whatever started it: a panel launcher, a
terminal, a program that double-forked to PID 1, a process that was there
but `exec`ed another program (its executable changed), or one that took a
recorded pid (its start time changed). Each is frozen with SIGSTOP until a
sweep finds no new one, so nothing forks while the rest are found; then
SIGTERM and SIGCONT, a grace of `TERM_GRACE` (3 s), then SIGKILL, repeated
for anything forked on the way out. Signals go through a pidfd checked
against the start time, so a reused pid is never hit. Autostart entries
made in the session are removed and changed or deleted ones put back; a
directory reached through a link is refused rather than cleaned through.
It answers ok only when a last sweep finds nothing, naming what it stopped.
Run again it sweeps again against the same boundary, so a retried handback
is checked afresh.

**Not stopped:** kernel threads, zombies, PID 1, the guest agent and what it
runs for the kernel (a preview screenshot during the takeover), and new
processes of a browser that was already running, reached from it through
processes of its own binary (`OWN_WORKERS`, Chromium): a new tab is a new
renderer, and killing it would crash the agent's tabs. Anything such a
browser launches of another binary, or a browser the person started, is
stopped.

**Fail closed.** The kernel ends the takeover only on the guest's ok. It
first cuts every open screen of that computer and refuses new ones while it
cleans up, so nothing is started behind the sweep (an open display stream
used to outlive the handback). Then: ok writes `TakeoverCleared` (what was
stopped and removed, shown in the computer's activity) with `TakeoverEnded`,
and the agent resumes. Anything else (an error, a timeout, a guest agent
restarted since and so without a boundary, an older guest) writes
`TakeoverUnclean` with the reason, keeps the person in control and the agent
paused, and tells the person to close what is left and choose I'm done
again, or reset the computer keeping their files. New event kinds, not new
fields on old ones.

**Why not a cgroup.** The brief suggested putting the operator session in
its own cgroup. There is no operator session process to put there: the
person drives the same X desktop the agent uses through VNC, so what they
start is a child of the panel, a terminal or the browser, which were there
before and whose children are the agent's too. A cgroup would hold the
whole desktop or nothing. The process table against a boundary tells the
session's processes apart; the freeze-then-kill sweep does what
`cgroup.kill` would. The image has no systemd, cron or at, and nothing
reads autostart entries (no session manager), so there are no units or jobs
to diff; the autostart entries are undone anyway in case one is added.

**What it costs the person.** An app they opened during the takeover is
closed at handback, after SIGTERM's grace; one that does not save on
SIGTERM loses unsaved work. The handback toast says how many were closed.
Windows the agent had open stay.

**Still not enforced.** Root in the computer can change files: replace a
program the agent or the guest runs (`/usr/bin/import`, the desktop script,
the guest binary itself, which then reports clean when restarted), add a
D-Bus service file, or install a browser extension into the running
browser. Those persist past handback. Passwords stay in the browser VM for
that reason. A kill switch that pauses a computer mid-takeover clears the
controller without a handback, so nothing is swept then.

Tests: `sandbox/guest/src/leftovers_tests.rs` (the sweep against a fake
process table: launcher, terminal, double-fork, exec, pid reuse, browser
workers, respawn, unkillable; autostart; boundary kept and retried);
`crates/magnus/src/computer/leftovers.rs` (only a plain ok is clean, guest
text capped); `one_tests.rs`
`a_handback_the_computer_cannot_confirm_clean_keeps_the_agent_paused`;
`display.rs` (a handback cuts the screens). The VM path is covered at
acceptance by invariant 18
(`tests/invariants/tests/inv2_credentials.rs`, takeover and handback on a
real computer), which now runs the guest's boundary and sweep.
