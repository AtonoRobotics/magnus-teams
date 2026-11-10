# Any use wakes a computer, and its panel always says who holds control

Spec rev 131 (Computer state: Lifecycle; User takeover and handback).

**Wake on use.** The kernel wakes a sleeping, stopped or reset computer whenever it is used: the agent's desktop tools (so a schedule's or a trigger's run too), the person opening its page or pinned screen (`POST /v1/computers/{id}/wake`, which returns at once), and taking control (which waits for the wake). There is no Start button. A run's computer is now its owner's computer in any wakeable state, a running one first (`wake::pick`); before, a sleeping computer sent the agent's desktop tools to the run's sandbox. One caller claims a wake; the others wait on it. A full server queues the wake as before, and the tool tells the agent its place in line.

**Not the kill switch.** A computer paused by the kill switch is not woken by any use, and is not offered to a run. Only its person's Resume undoes it. This is the safer reading: the spec's "any use wakes it" is about an idle computer, and the kill switch exists to stop use.

**Looking is not using.** `GET /screen` on a sleeping computer returns its last screenshot and the wake's progress and does not wake it, so a pinned panel left open does not keep a computer from sleeping. Opening the panel or page does wake it, once.

**The last screenshot** is kept per computer from every capture (the agent's actions, the person's preview, handback) and taken once more just before it hibernates. It is kept in memory only: a screenshot can show anything, and the spec promises nothing about one at rest on the host. After a server restart a sleeping computer shows no picture until its next capture; the wake's progress still shows. A deleted computer's picture goes with it.

**Wake progress** is in memory too (`starting`, `booting`, `restoring`), shown as `waking` on the computer and its screen, and as "Waking: …" over the last screenshot.

**One strip, two places.** The computer's page and the pinned panel draw the same screen and strip (`computerScreen`, `controlStrip`): who holds control and whether the agent is paused in one sentence, Take control or I'm done, and Full screen. The pinned panel takes control in place, so a card's Take control opens the pinned screen beside the conversation and takes it there, and the panel's markup never changes with the computer's state, so a redraw cannot cut a takeover off. Full screen puts the same frame (screenshot, preview or live display) into the browser's full screen; it is not a separate mode.
