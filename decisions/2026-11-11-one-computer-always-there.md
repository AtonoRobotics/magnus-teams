# One computer, always there

Spec rev 142 ("One computer, always there"), at the owner's request: every
person's agent has exactly one computer and each workspace's team agent has
one; nobody makes one or chooses between several; the agent's tools always
use it; destroy becomes reset; anyone who had several keeps one and is told.

**Kernel state.** The computer ledger keeps when each computer last had
anything recorded (`last_used`, from the event's time on replay). An owner's
computer is `Ledger::one_of`: its only one, or, while it still has several,
the one used last (then a running one, then by id, so every replay agrees).
`computer/one.rs` holds the rest.

**Made, never by hand.** `computer_for(owner)` returns the owner's computer and
makes it (computerd `create`, then `Created` by `kernel`) when there is none,
one maker at a time so two first uses make one. It is called:
- when a workspace is created (its route), and when an admin invites a
  person (the account is made then), in the background;
- by a look at start and every minute after (`reconcile_computers`), for every
  person the server keeps (config and local) and every workspace: safe to
  repeat, it makes only what is missing, and covers accounts made any other
  way within a minute;
- on first use by the agent's desktop tools, a new source, saving an
  attachment and asking for a takeover, which wait for it.
A server without computers makes none, and the desktop tools go to the run's
sandbox as before.

**No choosing.** `GET /v1/computers` lists the person's own computer and those
of the workspaces they look after, one each; an admin no longer sees every
person's computer there (that is Operations' job). `POST /v1/computers` no
longer makes anything: kept for clients from before, it answers with the one
there is (`{ id }`), for a workspace only to its admins. Answering 410 was the
alternative; the safer reading keeps old clients and the owner's invariant
tests working against the same promise, since nothing they get is a second
computer. The app's New computer button, the team panel's "Give Magnus a
computer", and the Delete button are gone.

**Always used, always woken.** The agent's computer tools resolve the run's
owner (its workspace on a team request, else its person), make the computer
if it is missing, and wake it (`use_computer`), so "You have no computer"
cannot happen on a server with computers. A team run is refused the desktop
tools only on a server without computers, not for a workspace "with no
computer". A computer paused by the kill switch is still the one: its use is
refused with the kill switch's words, where before the run was quietly sent
to its sandbox's desktop instead (the safer reading: the pause stops the
agent, it does not move it elsewhere). The client already shows the last
screenshot and the wake's progress, never a start button (2026-11-09).

**Destroy becomes reset.** Reset keeps the files by default (the dialog's
default button) or erases them. Erasing, or the old `kill` with
`delete_volume`, now replaces the computer: the new one is made first, then
one write revokes the old one's grants, deletes it (`Killed` with
`delete_volume`) and records the new one (`Created`), so the log never shows
the owner without a computer; then its vault entries go and computerd deletes
it, purging what it held within its purge window. Only the person it belongs
to may erase a person's files, as before. A plain destroy (the kill switch's
stop, files kept) stays: the computer is rebuilt on its next use. A deleted
workspace's computer goes by the usual deletion (`delete_computer`), under the
same one-maker lock so the look cannot make it again; one made in a race is
deleted by the next look, which deletes the computer of any workspace that is
gone.

**Several become one.** The look finds each owner with several computers and,
in one write, deletes all but the one used last (each by its grants' `Revoked`
and a `Killed` with `delete_volume`), records `Kept { id, owner, removed }`
(a new kind of computer change), and puts a notice in the inbox of the person,
or of each admin of the workspace: "You have now one computer, always there.
Magnus kept the one used most recently, last used <date>, with its files and
apps. N other computers were removed; everything on them is purged within 30
days." Then computerd deletes them (within its 30-day purge window, not an
immediate wipe beyond what the deletion already did). Once done there is
nothing to do again, so it is idempotent.

**Event history.** No event kind changed. `Created` and `Killed` with
`delete_volume` from before replay as they did; `Kept` is new.

**Tests.** `computer/one_tests.rs` against a booted kernel and a fake
computerd: everyone gets one on start and the next start makes none; a tool
call on a never-made, never-started and hibernated computer makes or wakes it
and goes on; erasing leaves a new computer, never none, and an admin cannot
erase a person's; several become the one used last, the person told once.
`desktop/tests/one-computer-check.mjs` (fast tier) and `team-check.mjs`: no
New computer, no Delete, no "Give a computer", reset defaults to keeping the
files. The autonomy acceptance has an eighth scenario, "the agent uses its
computer with nobody having set one up"
(`tests/scratch/tests/autonomy/own_computer.rs`).

**For the owner.** `tests/invariants/tests/inv8_team.rs` test 41 begins with a
workspace that has no computer and expects the team agent to be refused; a
workspace now always has one. The patch that brings it to rev 142 is in the
report of this change. Computerd's `max_computers` must allow one per person
and workspace: a server that hits it logs a warning each minute until room is
made.
