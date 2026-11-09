# Replay compat for delta history (#415)

`WatchedSeen.full` (serde default false) versions the
representation; the apply arm unions `full: false` events onto the
newest window entry and pushes `full: true` ones; both `note_watched`
emits set it (the no-news emit from #417 gets its flag at the
master merge, since this branches from pre-#417). Unit tests in
`state.rs` replay JSON history without the flag (authentic old
shape): deltas reconstruct `[{a,b},{a,b,c}]`, whole answers push
untouched. Delta test failed before, both pass after; workspace
check clean. Held unpushed until #417 merges (shared lines).
