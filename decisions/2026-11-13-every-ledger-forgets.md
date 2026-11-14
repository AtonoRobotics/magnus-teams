# Every per-person ledger forgets its own person

Erasing a person was one 66-line arm of `State::apply`: a hand-kept list
of retains over the state's own fields. Of the fifteen feature ledgers on
the state, only push was told. That list had been patched each time it
was found short (#191, #412), and it was short again: after an erasure the
state still held the person's rules with their words, their runs'
evidence read-backs (what was found), their spend requests with their
reasons, their purchases, their agreements, their own computers and the
files Magnus wrote there, their desktop programs, their workspace
memberships, whispers and read marks, their Team Admin role, the kill
switches on them, their connect-screen acceptances, their sub-agents and
their tasks' outcomes. Rule words, evidence and spend reasons are content
(`events::CONTENT`): the erasure deletes them from the log, but the state
in memory kept them until the next start, so they were still read and
shown. That was a real privacy gap.

- **Each ledger owns what erasing means for it.** A trait,
  `forget::Forget`, with one method, `forget(&mut self, who: &Erased)`,
  is implemented beside each ledger's records. `Erased` carries the
  person and what the state held of theirs just before (runs, tasks,
  items, threads, files), so a ledger keyed by run or task forgets those.
  A ledger that holds nothing of the person's says why in its `forget`.
- **The arm is one call.** `Ev::PersonErased` calls
  `State::forget_person` (`forget.rs`), which destructures the state
  field by field with no `..`. A field added to the state does not compile
  until it is listed there, forgotten or kept with its reason, and the
  feature ledgers are forgotten through one array of `&mut dyn Forget`.
  Reason: the list failed because nothing made anyone look at it; the
  compiler now does.
- **Also forgotten now:** the runs they stopped, what they moved, hid
  and removed of their own conversations, whether they signed in before,
  and the names of their secrets the erasure took from the vault (their
  app sign-ins, card and Link wallet).

What stays, and why:

- The audit and the erasures (the record that it happened, #195).
- Approvals, kill switches and spend decisions they gave or pulled on
  others: each was valid when given and is about the other person's work
  (#191). A kill switch is never lifted by erasing the one who pulled it.
- Invite links they made: the link is the server's, its maker a record
  of what they did.
- Their computer's record: it is what deletes the computer, at their
  removal or the next look (`computer/one.rs`).
- Their messages in a shared team channel: the channel is the
  workspace's record, the others keep their copy, and only a sender
  deletes a message (2026-11-11-deleting-and-archiving-messages.md).
  Projects and workspaces they owned are handed on or deleted before the
  erasure (2026-11-11-admin-sections.md).
- The organisation's own config, policies (a layer naming them is the
  admins' rule), apps (their built apps' records are harmless once the id
  is never given again, 2026-11-11-an-erased-persons-id-is-not-given-again.md)
  and knowledge.

The log is unchanged: no event kind is added or edited, and a replay
forgets the same way, since `PersonErased` replays after what it erases.

Test: `forget::tests::erasing_a_person_leaves_nothing_of_theirs_in_any_part_of_the_state`
seeds a record of every per-person kind for two people, erases one, and
checks that no part, and no line of the whole state as it prints, names
them, while the other's records stay. Before this change it failed on
the rules, evidence, spend and every other feature ledger. The state and
the outcomes ledger now derive `Debug` for it.
