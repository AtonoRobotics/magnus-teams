# Measured outcomes

The four signals the spec names are panels in the developer view, computed from the replayed log in `crates/magnus/src/measured.rs`.

- **Suggestions**: accepted, dismissed, not now, and regretted. A suggestion is regretted when what it started gets a verdict of wrong or undone, is stopped by a person, or when its schedule or grant is later removed.
- **Actions taken without asking, then reversed or complained about**: runs with an intent no one was asked about, whose task got the new verdict `undone` (reversed), or `wrong` or an annoying reaction (complained about).
- **Approvals asked and approved unchanged**: by tool and rule, an approval with no note attached.
- **Takeovers per task by cause**: `TakeoverStarted` now keeps what the agent was waiting on, or that the person stepped in, and the runs on the computer. Takeovers logged before this read as "not recorded".

There is no general undo, so reversal is what the person reports: "I had to undo it" joins the verdicts on a task. A count of a person's own later edits would guess at intent.
