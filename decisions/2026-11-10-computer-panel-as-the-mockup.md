# The computer panel follows the spec's mockup, and names the agent

The owner, on 0.7.1-test.50: "Computers and teams do not match the updated
spec." Measured against "User takeover and handback", "Installing and using
apps" and "Work on the user's desktop", and the mockup
(docs/design/team-and-computer/computer-panel.dc.html), the computer's screen
had the mechanics but not the look or the words:

- The state was said three times (Running, The agent is using it, The agent
  has control) and never named the agent or whom it worked for. Status in the
  spec is "whether the computer is active and which agent is using it".
- Full screen took the frame alone, so in full screen the person lost who
  holds control and "I'm done". The panel must always show both.
- Nothing on a working screen said that a click takes control, and the
  caption vanished once it was awake. Asleep and waking had only a caption.
- The conversation gave no sign the computer was in use and no way to pin it
  from there.
- The apps the agent installed (the manifest) were not shown anywhere.
- A connected desktop's mode and folders could be set only when connecting.

Decided:

- The server says, for each run on a computer, the agent's name, whether it is
  a sub-agent and whom it works for (`agent`, `sub`, `for`, `for_name`), and
  the computer's own name ("Your computer", or "<team> team computer"). Names
  are read after the state, from the accounts.
- One pill (Asleep, Waking, Waiting for room, Magnus is working, Magnus asks you
  to take over, You are in control, <name> is in control, Paused, Ready) and
  one sentence that says who holds control and whether Magnus is paused. The
  page's head keeps only the controls.
- The screen carries what the mockup draws: a veil with a moon while asleep
  (a click wakes it), a veil with a progress bar while waking or queued, a
  caption bar that is always there and ends in "click the screen to take
  control", and while the person holds it a bar on the screen itself, "You're
  in control. Magnus is paused." with I'm done, and a ring round the frame.
  Clicking anywhere on an awake screen takes control.
- Full screen takes the view (strip and screen), not the frame alone.
- The conversation's head shows the person's own computer as a chip ("Magnus is
  using your computer", "Your computer is asleep", ...) that pins its screen.
- The computer page gains an Apps tab listing the manifest: name, version,
  source and the hosts each app may reach, and a failed replay.
- A connected desktop's mode (ask every time, allow, never) and folders can be
  changed in place; the app restarts its connection with them. While the
  person uses the mouse or keyboard the page says Magnus waits, and offers to
  let it go on now. The list of connected desktops says the mode in force
  (`in_force`) when a team's or the admin's ceiling makes it stricter than the
  person's own choice, since the ceiling, not the choice, decides what asks.

Where the mockup and the spec differ the spec wins: the mockup's pill alone does
not say whether the agent is paused, so the sentence beside it always does.
