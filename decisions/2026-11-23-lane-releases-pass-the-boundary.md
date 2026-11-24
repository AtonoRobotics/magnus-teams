# A lane's automatic release passes the checks any send to other people gets

**Choice.** A finished lane's answer that would post itself into a shared
room goes through one function, `Kernel::release_automatically`
(`crates/magnus/src/team/lane_release.rs`). The lane watcher calls it for
every lane dispatched with `auto_release`: the asker's own `@Magnus`, a
routine's report (its author's own lane), an open-web lane, a standing
release, and each of them followed again after a restart. Quiet-thread
follow-ups are held already (`hold`), and a lane asked of someone else
without a standing release always waited in their Airlock. Neither changes.

Its checks run in order, and each can only hold:

1. The scrubber (`scrub_check`), as before.
2. The run's taint. A run that read content from outside through a tool
   (`Run::outside`: an email, a page, a message) while holding private data
   is held, whatever the models say. A member lane's run is always private,
   so for a member lane this means any outside read. The taint a run starts
   with when a teammate asked (`owner::from_others`) does not count on its
   own: a standing release covers that exact request (its key holds the
   objective), and the run already reads those words as data.
3. The outgoing classifier (`outgoing_finding`). A finding, or no answer,
   holds it.
4. Auto Review, through `auto_review_found` (the classifier's word passed in,
   so it is asked once). It judges a post of the content (tool
   `team_lane_release`, a reversible write) to the room's other members, who
   are named as its recipients and told to be other people. As in
   `asks::settle`, an allow acts alone where the owner's policy delegates or
   where the person set up what covers it (their standing release). Deny, ask,
   or no answer holds it.

Both model calls go through `reviewer_call_within` with their timeouts, so
they are booked to the lane's run.

A held answer becomes `LaneDraftReady` in its owner's Airlock with the reason
in a new optional field, `held_because` (content, so erasure removes it;
shown only to the owner, in the Airlock card and its home card). An event
written before it reads as no reason. A lane with no owner (open web) has no
Airlock, so it fails with a fixed reason, as a scrubbed one did.

A release the owner makes by hand from the Airlock is their own act. It
posts as before, with only the scrubber's refusal.

**A standing release is automatic, not the owner's hand.** The owner set it
once, for a request they had seen; each later answer is new content they have
not seen, produced by a run that may have read anything. That is the case in
the report, and the safer reading, so it goes through every check above.

**Reason.** The watcher's auto-release and `post_released_contribution`
skipped Auto Review, the classifier and the taint, and matched only
credentials, SSNs and card numbers. A standing release whose run read an
injected email could carry a salary or a contract term into the room. The
taint check rests on no model's word because
`2026-11-22-a-classifier-clear-is-not-a-pass.md` found that a model misses
private data that does not look sensitive and an attacker controls how it is
dressed; the classifier and Auto Review may add a hold, never remove one.
The room is a destination the person named, so this is stricter than the
boundary's rule for a send inside the envelope; a room's readers are other
people and a summary's content is exactly what an injection shapes, so the
stricter reading wins.

**What changes for people.** An answer posted unasked into a shared room now
waits in its owner's Airlock, with the reason, when its run read an email,
page or message, or when a check will not clear it. That includes a routine
whose report read the author's mail: the author releases it with one click.
An answer from clean work that the checks allow posts as before.

Tests: `team::lane_release_tests` (a tainted lane's private answer waits in
the Airlock; the classifier holds what a pattern misses; a clean answer still
posts; an unreachable Auto Review holds it and the owner's own release still
posts; a standing release goes through the checks).
