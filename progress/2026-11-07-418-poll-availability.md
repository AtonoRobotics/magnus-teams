# Poll availability check (#418)

`poll_due` iterates `self.connectors()` and skips `!connected ||
lapsed` per person, mirroring `app_tool_specs`. Three lines plus
doc.

Test in `watch_poll.rs`: mail (connected) plus cal (secret
withheld via `without_secret`, never connected) plus cal2 (turned
off through the admin API before the loop starts — boot with the
loop off, PATCH enabled=false, reconfigure on, the cadence-test
dance, so no race). The model records poll-shaped calls per app and
ends polls without reading. After 20s past the first due sweep:
mail polled, cal and cal2 never did. Failed before (cal polled),
passes after. Full watch_poll 2/2; gate: proactive 9, reviewer 6,
new_items 3, lib 116, inv4 11, inv7 5, workspace check clean.
