# Erasure cancels a person's runs through cancel_trees

**Choice.** Erasing a person ends all their runs still going in one write through `cancel_trees`, with every sub-agent below them, as every other cancel does. A failed write is logged and the erasure goes on, as before.

**Reason.** Erasure cancelled each run in its own write and left the sub-agents below to the run's end, in a later write: a reader could see a run of theirs ended and a child of it still going, and a child the parent's end already cancelled was cancelled a second time. `run_end.rs` says every cancel takes the one path.
