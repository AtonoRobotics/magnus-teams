# Taking over a computer

**The controller is in the log.** `TakeoverStarted` and `TakeoverEnded` name who holds the screen. A new holder replaces the last, because the user always wins. A kill or a hibernate ends the hold.

**The agent stops.** A run that has used the desktop tools suspends with reason `takeover` at its next step, before any model call or tool. Should a desktop tool still reach execution, it is refused and captures nothing. Handback writes a notice and a fresh screenshot into each such run and resumes them. Screenshots are content, kept beside the log like any tool result's image.

**The display is reached one way.** The guest serves VNC on loopback, and a thread bridges vsock port 5900 to it. computerd's `display` opens that through the same jailed relay as the guest agent. Magnus's WebSocket route carries the bytes. No port is open on any network.

**Tickets.** `POST /takeover` returns a 256-bit ticket that lasts 30 seconds and works once. It travels as a WebSocket subprotocol (`magnus.ticket.<value>`), because a browser can set that without putting it in a URL. Only its hash is kept. Presenting one spends it, even when it is refused. A caller who is also signed in must be the person it was issued to.

**Screen and files are their user's.** The content routes now check `opened_computer`: the person it belongs to and no one else, so an appliance owner or admin no longer reads them. Those two keep kill, hibernate, backup and the rest of `may_control`. An operator reads through `/v1/operator/computers/{id}/{files,screen}` only in a session its user granted, for at most 8 hours. Each look is logged before anything is shown, and `/activity` shows it to the user.

**The agent's tools.** `computer_*` act on the person's running computer when they have one, and on the run's own sandbox otherwise. `add_computer_destination` is a boundary action reviewed by Auto Review.

**Idle.** A computer with no use for `idle_hibernate_minutes` (default 30), no controller and no running run of its person's hibernates. A schedule or a person wakes it.

**Not here yet.** Pausing when the user merely moves the mouse needs input events the broker does not read. The status and preview levels, and the client view, arrive with the UI work. A process that root left running in the computer can still watch the screen, which is why passwords go in the browser VM.
