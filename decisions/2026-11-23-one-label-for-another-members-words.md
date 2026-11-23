# 2026-11-23: One label for words from another member

## Choice
`team::owner::from_others(person, writers)` is the one rule for whether words a run is given came from anyone but the person it acts for, which makes the run untrusted. A whisper's thread, a member lane asked for by someone else, a steer of such a lane, and a knowledge-base read all use it. A knowledge-base entry written (or owned) by anyone other than the reading person now marks the read untrusted, as an entry from outside already did. A writer is compared as written: an `agent:<person>` sender is not the person, so their agent's posted output stays untrusted as before.

A steer of a lane someone else asked for now starts its run as that lane did, with the requester as the actor, so the task is `by_other` and its words read as `Source::Other`, never as the owner's own.

## Reason
Whispers, cross-user lanes and steers already marked another member's words untrusted, but every knowledge-base entry added through the route weighs as `Member`, so a teammate's entry reached another member's personal run as trusted text and could steer it. A steer put the requester's objective inside the owner's own request, so it read as the owner's instruction. One function for the question keeps the four paths from drifting apart again.

A curator's correction of someone else's entry keeps the entry's original author and owner; such a correction read by that author is still counted as theirs. Tracking who last wrote an entry's statement would close that, and is left for a change that records it.
