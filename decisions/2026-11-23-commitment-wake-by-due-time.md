# A commitment wakes one review for each time it is due

**Choice.** The proactive loop marks a commitment's wake with the commitment's due time as recorded (its `by`), not with the day of the wake. A review wakes for an open commitment due within the next 24 hours (`COMMITMENT_AHEAD_HOURS`) unless one already woke for that due time. A changed due time wakes a review again. Schedules, stale approvals and ended tasks keep their per-day marks.

**Reason.** The window is 24 hours but the mark was per calendar day in the person's zone, so a window crossing their midnight woke two reviews for one due time.
