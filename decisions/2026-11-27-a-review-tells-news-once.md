# A review tells the person news once

The owner, 2026-11-27, saw many duplicate observations under Tasks and
asked why one ran 37 times in 12 hours. In production that day the
reviews queued 57 observations in 24 hours, nearly all the same news in
other words: two schedules paused after three failures, and a Slack trial
ending. 93 reviews woke in 12 hours, 36 of them on the cadence, the rest
for new mail. Every review read the same paused schedules, and nothing
showed it what earlier reviews had already said.

Choice:

- The review's context lists what it already told the person: their
  observations and interruptions of the last 7 days (`ALREADY_TOLD_DAYS`),
  open or dismissed. The review instruction says that a state that has not
  changed is not news.
- `observe` and `interrupt` refuse news already told. The check is either
  the same words (`same_fact`), or an observation that names only
  schedules, tasks, items or runs an earlier one already named, counting a
  shortened id as a match. Nothing is queued, and the model is told why.
  News that names something new, such as a fresh failure, is queued.
- A cadence review starts only when the state a review reads has changed
  since the last review started: unfinished work, schedules, approvals,
  disagreements and commitments. What reviews told the person is not part
  of that state, so an observation never wakes the next review. Reviews for
  new mail, ended tasks, due commitments, coming-up schedules, waiting
  approvals and the overnight review are unchanged. After a restart, one
  cadence review runs, because the last state is kept in memory only.

Reason: an observation is news queued for the person's next session, as
the spec describes. The same news fifty times is noise, and each telling
was a full review run.
