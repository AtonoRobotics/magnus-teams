# The person's own computer: network, macOS, yielding, capture

A command has no network unless it asks for it, and asking makes it a send: the same check as anything else that can carry data away. That is what lets an install run while keeping data read on the machine from leaving unasked.

On macOS a command runs under `sandbox-exec` with a profile that allows only the system, the granted folders and the network if asked. The keychain, its security server and home directories are not listed, so they are refused. The profile is unit tested here; it has not run on a real Mac.

The agent yields while the person is using the machine. Yielding is decided on the machine, by the system's idle time or by the app's pause file. A job waits in the server's queue, not on the machine, so a job given up on cannot run later. "Go ahead" lasts ten minutes.

The screen, camera and microphone are each one tool that asks every time unless a rule allows it. The operating system prompts at the moment of use because nothing touches them before. A recording stays in a granted folder, because the agent cannot listen to it.

Where the system has no idle time to offer, the agent is not held back: the person can see it and stop it in the app.
