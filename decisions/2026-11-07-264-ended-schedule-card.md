# An ended schedule stays, marked with why (#264)

`ScheduleEnded` removed the schedule as `ScheduleRemoved` does, so the
card could only say Cancelled and the listing lost the latest run.
Cards render from the object's current state: an ended schedule now
stays in state with `ended` set to the event's reason, and only a
cancelled one is gone.

The end is terminal. The projection clears any pause (nothing to
resume), `finished()` answers none once marked (the sweep never
re-ends), pause, resume and admin suspend refuse on an ended schedule,
and the UI offers no acts on one — only Open and Its runs. An edit
revives: edits only move the end forward, so clearing the mark is
always correct, and the sweep re-ends if it is still past.

The card says Ended with the reason and the latest run through the
existing `last` linkage; the list sorts ended last and shows the same
mark. Live rendering is byte-identical.
