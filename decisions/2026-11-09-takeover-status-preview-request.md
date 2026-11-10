# Takeover: who is driving, the pinned screen, and the agent asking

The spec's User takeover and handback names three levels (status, preview,
takeover) and an agent-requested takeover. Takeover and handback were built
(`2026-11-08-computer-takeover.md`); these finish the rest.

**Who is driving.** A computer's JSON carries `driving`: `person` while
someone holds the screen, `agent` while a run that has used the desktop tools
is running, `no_one` otherwise, with the runs on it and any open request. The
computers list, the computer's page and the pinned panel say it in words and
name the tasks using it. It is computed from the log, so it needs no event.

**The pinned screen.** The panel beside the conversation has a `computer`
view: the status, any request, and the screen, refreshed every 3 seconds
while it is open. "Pin beside the conversation" on a computer's page and
"Show the screen" on a request open it. "Take control" anywhere opens the
computer's page and takes over at once, as the spec's full-screen takeover.

**The agent asks.** `computer_request_takeover` takes a reason from the
spec's list (single sign-on, passkey, CAPTCHA, an approval on another device,
a hardware security key, a password change) or `other`, and a `why` of at
most 500 characters. It writes an inbox item of kind `takeover_request`, the
same way a secure form is asked for, so the request is in the log, the inbox
and the conversation without a new event kind. Its risk is `read`: it changes
nothing, and asking again while one is open returns the open one. Background
work has no one to ask and is refused, as a sign-in is.

**The run waits.** While its request is open a run suspends with reason
`takeover_request`. A person who takes control and hands back answers it
(`handed_back`), and the run resumes with the usual notice and fresh
screenshot. From the card the person can also answer `done` (they did it
another way, such as on their phone) or `decline`; either resumes the run
with a notice saying which.

**Measured outcomes.** A takeover the agent asked for is recorded with the
cause `the agent asked: <reason>`, so takeovers per task by cause split by
what the agent could not do.

**Not here yet.** Pausing the agent when the person merely moves the mouse
over the preview, without taking control, still needs input events the
broker does not read.
