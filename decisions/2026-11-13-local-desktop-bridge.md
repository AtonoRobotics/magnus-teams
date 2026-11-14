# Local desktop programs, phase 1: the bridge

GAP10's first phase (decisions/2026-11-13-remaining-review-gaps.md) is
built with no program's own server yet. What it chose, and why:

**The channel is the app's own long poll.** The desktop app reports its
computer to `POST /v1/desktop/devices` and takes calls from
`GET /v1/desktop/devices/{id}/next`, with the session it signed in for and
the client it already pins (or its device certificate on the public name),
as the local agent does. The app listens on nothing for it, so a call can
come from no one but its own server. Only a bearer session opened by the
app (`client == "app"`) reports, polls or answers, and only for its own
person's computer: a browser's cookie, or another person, is refused.

**A call names a program, never an address.** The server's job carries the
program id, the tool and its arguments. The app sends it to the loopback
port registered for that program (`bridge.rs` `Ports`: 127.0.0.1 or ::1
only, a supported program only), and only while the person has that
program enabled on this computer. Phase 1 registers no port: each
program's own phase names its server's port.

**A computer is the id the app makes.** Pairing certificates exist only for
the public name, so the app makes an id once and keeps it in its settings
folder; the server ties the id to the first person who reports it and
refuses another's. The computer is named by its host name.

**Consent is a new event kind, `desktop`** (`Device`, `Removed`,
`Consent`), one consent per program per computer. Allowing is done only
from the app (an app session); taking it back from anywhere the person is
signed in, since stopping is never riskier. Removing a computer takes its
consents with it. What the app found (versions, paths, whether a server
answers, its tools) is not kept in the log: it is the computer's word of
the moment, held in memory and reported again each round.

**State per program per computer:** not installed (not reported), not
enabled (no consent), closed (enabled, but its server does not answer, or
the computer is not connected now), ready. The agent sees a ready
program's tools as `desktop_<program>__<tool>`, with `computer` listing
the computers it is ready on, and `desktop_programs`, which says where
each program stands on each computer. The person sees the same in Apps.

**Policy.** A program's tool is classed by the server's catalog
(`desktop.rs` `PROGRAMS`); a tool it does not class is taken to run code
inside the program (the safer reading) and is irreversible, so the default
organisation policy asks. For a program's tool the policy's own ask stands
as a person's ask (case 4 of the four asks, rule `kernel:local_program`),
not only Auto Review's: the connectors spec says the default requires the
user's approval, and it lets a person's forbid of `execute` make a
program ask them first. The organisation's layer relaxes it, for everyone
or a group (`principal in Group::"..."`); a person's layer can only forbid
(ruling 5, `policy_layers::tightens_only`), and a permit there changes
nothing when decided. Results are labelled untrusted and private; a call is
a reviewed send like the local agent's.

**Detection** runs on start and again every minute (installs change
rarely; a watcher per system is more code for a minute's gain). It reads
the package records, Flatpak and Snap listings, the PATH and install
paths on Linux, and the uninstall keys (`reg query`) and install paths on
Windows. It never runs a program to ask its version. Both detectors build
and are tested on every system with a fake computer and registry; only the
system's own runs.

## What remains of phase 1

- Supervise: start a program's server, launch a closed program when policy
  allows (else ask). The bridge only sees whether the server answers.
- Install the pinned add-on from the catalog on Enable; for Fusion, show its
  one setting and confirm the port answers.
- Files a program produces (STEP, STL) returning as tool results: text
  results only now.
- Tool definitions from a program are not pinned and screened like a remote
  connector's (B10): they are labelled untrusted and prefixed with where
  they come from. Pin them when the first program's tools are classed.
- The agent's app index does not yet carry "on this computer"; the agent
  learns it from `desktop_programs` and the tools offered.
- Each program's port, and its tools' classes, come with its phase
  (Blender and FreeCAD next).
