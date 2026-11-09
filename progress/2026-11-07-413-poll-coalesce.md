# Poll/read coalescing (#413)

`note_watched` emits `WatchPolled` for every watched answer, so
task reads postpone polls; the poll-start mark stays as the retry
bound for polls whose reads never answer. One emit plus doc.

Test in `watch_poll.rs`, timed from the first poll: everyone reads
at T0+15s, and nothing polls in (T0+55s, T0+70s) — the old schedule
would fire at T0+60s. Failed before (six in-window polls — which
also exposed #425, the double-start race), passes after in ~71s.
Full watch_poll 3/3; gate: proactive 9, reviewer 6, new_items 4,
lib 118, inv4 11, inv7 5, workspace check clean. Held unpushed
until #424 merges (same test tail).
