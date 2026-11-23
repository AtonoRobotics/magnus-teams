# Removing an item reaches the runs that read it

Deleting a memory, a file or a knowledge entry removed the item and its writing run's copies. Every later run that searched or read it kept the item's text in its own tool result, in the log and in the backups.

The owner's ruling, 2026-11-23, approving the review's recommendation: yes, text a run only read out of a deleted item goes too. The review put it: "a deleted 'my home address' memory should not survive in every run that looked it up." The rule "only the item goes; what it made stays" still keeps what a run made from what it read (its answers, its files, its memories).

How (`read_copies`, the same idea as `team/copies.rs` for team words): memory search, knowledge search, project file read and playbook search say in their result's data which items they returned (`read_items`; a knowledge search's `entries` and a project read's `file` from before are read too). Removing an item finds those results in the state (`State::reads_of`) and names them in its `Removed`, so their content leaves the log and the backups with the item's. A memory search from before results said what they returned is found by the fact's text in the results of its person's runs that searched memory. When the `Removed` is applied, those results lose their text in the state as well, live as after a replay.

Lessons are read the same way and named in their results; a lesson leaves only with its person's erasure, which takes all of their runs' content.
