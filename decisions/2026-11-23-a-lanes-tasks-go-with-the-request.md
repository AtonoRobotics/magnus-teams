# 2026-11-23: A lane's tasks go with the request that made them

**Choice.** A mission lane's task and every steer of the lane are tasks
of the lane's owner whose text quotes the requester's objective (and, for
a lane, their name). `team/copies.rs` now names their `TaskCreated` rows
as copies: erasing the requester takes them (`Kernel::copies_of_person`,
for each mission they asked for), and so does deleting for everyone the
`@Magnus` message that started the mission (`copies_of_message`). The
whole text of each row goes, the owner's own edit in a steer with it, as
a whisper's old row goes with any message it quoted.

A lane names its own task (`child_task`). A steer is linked to nothing,
so it is told by how its text begins: `copies::steer_prompt` writes it,
and the copies look among the owner's tasks written after the lane was
dispatched for that opening with the lane's objective. Steers older
releases wrote begin the same way, so they are found too, and no event
kind changes.

**Reason.** The spec's erasure deletes a person's content wherever it is,
and a delete for everyone takes a message's words out of the log and the
backups. These two rows are the copies the earlier decision
(2026-11-23-one-owner-for-words-and-their-copies) left out. Recording a
link for each steer would need a new mission event (which an older
release could not read on a rollback) or a change to an existing one;
the opening finds old steers as well as new ones.

**Not changed.** What the owner's run made of the request (its replies,
its draft) stays the owner's.
