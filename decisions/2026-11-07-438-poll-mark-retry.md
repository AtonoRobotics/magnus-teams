# Poll marks retry a hiccuping event store (#438)

Split from #425: start_poll and note_watched ignored the
WatchPolled emit result, so a failed Postgres append lost the mark
and the watch re-polled every sweep. Unobserved in probes (marks
landed 3/3), but one outage away from a poll storm, and silent at
that.

Fix: both mark sites emit through a three-attempt helper with
100/200ms backoff, warning loudly on final failure instead of
dropping the mark. Retry at emit level is safe: numbering only
touches bundle versions (recomputed identically per attempt), and
check_contiguous tolerates the sequence gaps failed inserts leave.
The helper takes the emit as a closure so the unit test drives it
with a scripted store; the existing #413 window test guards the
wiring (no marks at all fails it, as the #425 mutation check
showed). Window-event emits in note_watched keep their shape:
marks only, per the issue.
