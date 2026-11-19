# An outage asks no one; the Rule of Two stays deterministic

From the review of approvals for ceremony
(`docs/proposals/2026-11-19-approvals-review.md`, items 1, 2 and 3 (item 5 was tried and reverted, below),
and the owner's card model), all accepted by the owner on 2026-11-19. The
standard: protect the person, their data and Magnus as autonomously as
possible, asking as little as possible, without giving up safety that is
useful and necessary.

**An outage is not a decision.** Auto Review, the outgoing classifier and
the safety monitor are each tried up to three times with a doubling wait
(`review_outage`, `REVIEW_ATTEMPTS`, `REVIEW_FIRST_BACKOFF`). If Auto Review
or the monitor still cannot answer, the action is not done, the agent is
told it may try again shortly, and nobody is asked: the new
`auto_review::Verdict::Unavailable` and `safety::Watch::Unavailable` deny
with a retry. Before, each outage put a card in front of the person, who had
nothing more to judge than the system had and would allow it blind. It
stays fail-closed: nothing goes unreviewed. An action too large for Auto
Review to read is refused the same way, and the agent is told to split it.
An unavailable verdict is never cached.

**The outgoing classifier advises.** Its finding is input to Auto Review.
When it cannot answer, Auto Review is told so
(`CLASSIFIER_UNAVAILABLE`) and reads the content itself, instead of the
action becoming an ask.

**The send reviewer's flag goes to Auto Review.** The reviewer module calls
itself "quality, never a security control" and lets everything through when
it is down; its flag put a card in front of the person. Now the flag goes
to Auto Review with the send ("the send reviewer flagged it: …"), which may
still ask. Its block stands.

**The Rule of Two stays deterministic (tried and reverted the same day).**
Item 5 had a write that names no recipient ask after untrusted input only
when the outgoing classifier found private data in it. The injection suite
(`tests/scratch/tests/injection.rs`, run on GitHub) caught it at once: an
injected instruction posted the person's private canary to the team chat
after reading their mail, because the classifier saw nothing sensitive in
it. A real classifier fails the same way on private data that does not look
sensitive (a token, an id, a code), and the Rule of Two holds against an
adaptive attacker only because it does not depend on a model's judgement.
So such a write still asks once after untrusted input and private data;
Allow for this task covers the rest of the task
(`decisions/2026-11-19-allow-covers-the-task.md`), which is what removes the
repetition.

**Allow always after untrusted input is for what can be undone.** Case 3's
card offers Allow always only for an action that can be undone, to a named
destination; what cannot be undone is allowed one task at a time, as the
owner's model has it.

Invariant 4's tests change with this (an outage of the classifier no longer
asks; Allow always only for what can be undone). The patch is `docs/proposals/2026-11-19-approvals-invariants.patch`,
for the owner to apply with `git apply`.

Tests: `review_outage::tests`, `safety::tests` (`watch_of(None)` is
unavailable), `asks::tests::the_card_is_one_sentence_three_answers_and_no_reasoning`,
`tests/scratch/tests/reviewer.rs` `a_flag_goes_to_auto_review_not_to_the_person`.
