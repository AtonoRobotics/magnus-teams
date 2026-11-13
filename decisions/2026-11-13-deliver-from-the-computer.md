# A delivery reads from the agent's computer too

The agent works in two places: the run's sandbox, and its persistent
computer, where the apps it installs run (Blender, FreeCAD). `deliver_file`
read only the sandbox. A CAD package the agent made on its computer never
reached the person. The task reported a path on the computer and asked the
person to sort out the transfer, which a non-technical person cannot do
(owner's conversation, 2026-11-13).

Choice: `deliver_file` takes an optional `from` (`computer` or `sandbox`).
Without one, it looks first where the run has been working: the computer
when the run used computer tools and no sandbox tools, the sandbox
otherwise. Then it looks in the other place. On the computer the file must
be under `/workspace/`, the person's files, which the computer's own file
routes already use. It is read in 4 MB pieces, up to the same 25 MB a
delivery has always carried. A path in neither place is one plain error
naming both.

Why not a storage connector: the hard part was getting the file off the
computer, not where it goes afterwards. Once delivered, it is the person's
like any output.

Test: `tests/scratch/tests/computer_deliver.rs` (real computerd, Strix Beta).
