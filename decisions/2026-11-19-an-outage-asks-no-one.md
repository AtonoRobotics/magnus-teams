# An outage asks no one; the Rule of Two judges what goes out

From the review of approvals for ceremony
(`docs/proposals/2026-11-19-approvals-review.md`, items 1, 2, 3 and 5,
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

**The Rule of Two judges what goes out when no recipient is named.** After
the work read untrusted input and private data, an app write that names no
recipient (a commit, a comment, a channel post, a deploy) asked every time
as "a new destination", though the card could name none for the person to
judge. The threat is real (injected text steering private data into a
public place), so it is kept, aimed at what it protects: such a write asks
only when the outgoing classifier finds private data in what it sends. If
the classifier cannot answer, the write waits, as above. A write to a
recipient, a web or browser lookup, and the person's own computer keep the
rule as it was. Auto Review still judges every such write.

**Allow always after untrusted input is for what can be undone.** Case 3's
card offers Allow always only for an action that can be undone, to a named
destination; what cannot be undone is allowed one task at a time, as the
owner's model has it.

Invariant 4's tests change with this (an outage of the classifier no longer
asks; a case-3 test's post now carries data the classifier flags; a new
test for a post carrying nothing private; Allow always only for what can be
undone). The patch is `docs/proposals/2026-11-19-approvals-invariants.patch`,
for the owner to apply with `git apply`.

Tests: `review_outage::tests`, `safety::tests` (`watch_of(None)` is
unavailable), `asks::tests::the_card_is_one_sentence_three_answers_and_no_reasoning`,
`tests/scratch/tests/reviewer.rs` `a_flag_goes_to_auto_review_not_to_the_person`.
