# Someone else's work is stopped, never changed, and its person is told

Spec rev 145, Administration, Agents: "An admin can pause or stop any one of them ... An admin sees what an agent did, from the audit log. Seeing a person's content follows the person-data rule above" ("Every admin read of a person's data is logged, and the person is told"). The owner, 2026-11-11: "users should be isolated from each other securely".

The control rule (`controls::may_control`, and the default control policy) let the owner and any admin cancel, pause, resume, edit and delete anyone's tasks and schedules from the app's routes and from chat. Only a pause left a trace the person could see, and a cancel answered with the names of the tools the person's run had used.

Now, whatever a policy layer says (`OTHERS_MAY`):

- An owner or admin may cancel another person's task and pause their schedule, as stopping is always safe, and lift a suspension an admin put on a schedule.
- Editing a schedule's timing, resuming one its person paused, and deleting one stay that person's.
- A stop by someone else puts a notice in the person's inbox naming who did it, and the answer to the one who stopped it no longer lists what the run had done.

Reading other people's task and schedule lists through chat is unchanged: it is already logged and the person told (`record_data_read`), and the server's data-access switch can make it private.
