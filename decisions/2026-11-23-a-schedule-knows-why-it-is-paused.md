# A schedule knows why it is paused

**Choice.**
- A schedule keeps a set of pause causes (`PauseCause` in `state/arrangements.rs`), each with what was said of it. A pause adds its cause, a release lifts only its own, and `next_due` is None while any is left. `paused`, `paused_on` and `paused_by_admin` stay as fields kept from the set, so their readers and the wire are unchanged.
- Three causes, read from the existing events' fields, with no new event kind and no new field:
  - a `SchedulePaused` with a `task` is a hold on that waiting run;
  - one marked `admin` is an admin's suspension;
  - any other is a pause for its person: they (or someone over them) paused it, its runs failed in a row, or they were turned off. All three last until the person resumes it, so they are one cause.
- A `ScheduleResumed` lifts: from the kernel with no source, the hold (the sweep's release); from the console, the admin's suspension; from the schedule's person, their pause and a suspension (the app lets them lift that only when they administer); from anyone else, a suspension.
- Turning someone off adds the person cause to every schedule of theirs not over, paused or not, except one already paused for them.
- The person may resume a schedule held on a waiting run that they also paused: their pause lifts and the hold stays until they answer. A resume of a hold alone is still refused with the old words. The admin console's resume lifts only an admin's suspension, and says so when the schedule is paused for its person or held.

**Reason.** All four causes shared one flag, and each pause overwrote the last. Turning someone off skipped a schedule held on a waiting run, and when they came back and answered it, the sweep's release resumed the schedule and it fired, against this morning's decision that a turned-off person's schedules stay paused until they resume them.

**Note.** A `SchedulePaused` marked `admin` is an admin's suspension even when it names a task (no release writes that pair; the pinned state sequence does). The pinned state rendering gains the `pauses` field, and that sequence's one such pause now leaves `resumed` at its resume, as any lifted suspension does.
