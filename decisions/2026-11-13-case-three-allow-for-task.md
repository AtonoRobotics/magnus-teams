# An Allow on a new-destination ask covers the rest of the task (owner approved, 2026-11-13)

**Choice.** Case 3 of the four asks (private data to a new destination after untrusted input) offers what every other card does: Allow covers the rest of the task for the same tool and destinations, and Allow always saves a narrow rule. Only an Allow given on a case-3 card covers case 3. A payment still asks every time. The patch, `2026-11-13-case-three-allow-for-task.patch`, holds the kernel change, its unit tests and invariant 4's test.

**Reason.** The spec, How an ask looks: "Allow covers the rest of the task: later actions of the same kind to the same destinations go ahead. Allow always saves a narrow rule." Case 3 asks "unless a rule covers it", and "an approved destination is remembered". The kernel offered case 3 only a one-time Allow, which the owner reported (2026-11-13): "There is no always approve or approve for task."

**Approval.** The owner, 2026-11-13: "apply it." Applied with invariant 4 retargeted: case 3 still asks before anything goes; its Allow covers only the same tool and destinations.

**Applied** in `9254753` (2026-11-13); invariant 4's test was later shortened in `d66fbef`. The patch file was removed on 2026-11-13 after checking its content is in the tree.
