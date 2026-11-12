# An ask goes to its requester alone

The owner saw, and answered, an approval card for another member's task
(hartenfelsj's `web_fetch` of a festival site, 2026-11-12 00:25 UTC). In the inbox,
`may_respond` still followed the approval model from before rev 140: outside a
team thread, every approval was shown to, and could be answered by, the
server's owner and anyone holding an `approver:<risk>` role. `person_cover` then
accepted the owner's answer as cover for the member's action. It was the only
such approval in the log.

Spec rev 145, Approvals: "An ask goes to the requester, or to the approvers a
workspace rule names when the action uses a workspace resource. Its card shows
only to them, never to the rest of the thread." Rev 140 "replaces every earlier
approval and task rule". Administration allows an admin to read a person's data
only when the read is logged and the person is told; an approval card is neither.

Choice:
- An approval card is visible to, and answerable by, its requester only. For
  everyone else it answers 404, as for an item that does not exist. The
  team-thread special case went with it, because the rule is now the same
  everywhere.
- An approval covers an action only when the run's own person gave it. Approvals
  that an owner or approver gave for someone else, including the one above,
  cover nothing from now on.
- Approvers named by a workspace rule are not built yet. When they are, they
  come in through the item itself and not through server-wide roles.

The roles `approver:<risk>` still gate whether a person's own rule may stand in
for them (`person_cover`, `auto_review`). That is a person's own capability, not
access to someone else's work.

Tests: `tests/scratch/tests/people.rs`
(`asks_go_to_their_requester_alone_and_policy_rights_follow_the_layer`):
the owner, a financial approver and an irreversible approver neither see nor
answer carol's asks, and carol does. `review.rs` and `turned_off_apps.rs` were
updated the same way. The invariant `only_holders_of_the_approver_role_can_approve`
encodes the old model; the owner applies
`inv8_layers-asks-to-requester.patch`, which replaces it with
`an_ask_is_answered_by_its_requester_alone`.
