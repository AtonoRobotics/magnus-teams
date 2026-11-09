# Old delta events replay as deltas (#415)

Codex P2 on #411, confirmed: history before #411 holds fresh-only
ids after the seed, but the window arm replays each event as a
whole answer. `{a},{a,b},{a,b,c}` persist `{a},{b},{c}`; replay
kept `[{b},{c}]`, so an unchanged inbox woke once for `a` after
upgrade (then self-healed as new whole-answer events arrived).

`WatchedSeen` gains `full` (serde default false, so history reads
as deltas with no migration): the arm unions a delta onto the
newest entry it extends — reconstructing the answers when the seed
precedes them, degrading to the ids alone when the window is empty
— and pushes whole answers as they come. New emits set `full`. No
CONTENT change: the flag is not content, and #410's migration
builds no event literals. Mixed windows (old then new) behave: the
first whole answer after history pushes complete.
