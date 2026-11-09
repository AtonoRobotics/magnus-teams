# Watched answers restart the poll clock (#413)

Since #406 the poll clock advanced only when a poll started: a
task read seconds before a due poll did not postpone it, so the
poll re-read what was just read. Every watched answer now emits
`WatchPolled` alongside its diff, so "polled on schedule" means
"fresh within the interval", whoever kept it fresh — and a wake
dropped for an active review retries on the next poll even when
nobody reads, which was the other half of the issue.

The start mark stays: a poll whose read never answers (error,
suspension) must not retry every sweep, so `start_poll` still
marks at start and the answer mark only ever moves the clock
forward from there. One more small event per watched read, the
same price #414 already pays for the window. Polls that answer
mark twice (start, then answer seconds later); the second mark is
the truer clock.
