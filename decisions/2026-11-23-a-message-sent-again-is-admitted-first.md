# A message sent again is admitted first

**Choice.**
- Editing a message or retrying an answer is one chat-turn operation, `Kernel::send_again` (`chat_turn.rs`). It admits the new turn first, through `Kernel::admit_new` (the switches, the person's model access, the month's spend limit), which `create_run` now also goes through. A refusal returns before anything is written, with the same status and words a plain new message gets (409 for a switch, 402 for the spend limit, 403 for a model).
- Once admitted, the hiding of the message and what came of it, the cancels of the work it started (and the sub-agents below that work) and the new turn's `TaskCreated` are one write (`cancel_trees`, the several-run form of `cancel_tree`).
- Only the thread's own work is cancelled: a task is superseded only when its `chat.thread` is the thread the message is sent again in. A branch keeps the original thread's task ids in its copied entries, and the original's work is not the branch's to stop.
- The new turn reads the thread as it will show after the hiding, and carries the taint of that, as before.

**Reason.** The old order hid the message, cancelled the running work and only then asked whether a turn could start. A person whose agent was paused or whose month was spent lost their thread and their running tasks and got nothing back. And in a branch, a retry picked runs by task id alone and stopped work still running in the original thread.
