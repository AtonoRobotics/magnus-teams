# A computer the machine stopped is recorded stopped

The owner: the team computer opened, but their own computer said "The screen could not be opened: HTTP error: 500". The server had restarted (2026-11-11 20:32), and the person's computer, running before it, died with it. The log still had it Running, so opening its screen asked computerd for a VM that was gone, and the idle loop failed to hibernate it every minute ("is not running"). The team computer had been asleep, so it woke normally.

Choice: once a minute the idle loop asks computerd the state of every computer Magnus has running. One that computerd reports stopped, or hibernated when a snapshot was kept, is recorded with a new event kind, `computer.found_stopped { id, hibernated }`, and the next use wakes it. It holds the hibernation claim while it asks, so a wake that is booting or a hibernation under way is never mistaken for a stop. A running or paused answer, or no answer, changes nothing.

Check: `computer::idle::tests::the_machines_answer_decides_whether_a_computer_stopped`.
