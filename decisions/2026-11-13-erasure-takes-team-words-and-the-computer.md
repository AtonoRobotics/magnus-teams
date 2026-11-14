# Erasing a person takes their team messages and their computer

Spec rev 147, Admin → People: "Removing a person transfers or deletes
their projects and agents and deletes their data". The erasure merged in
`2026-11-13-every-ledger-forgets.md` kept two things of the person's and
asked about both. The owner, 2026-11-13:

- "Should an erased person's messages in shared team channels also be
  erased?" — "yes — this was already decided and supposed to have been
  fixed."
- "A person defined in the config file who is erased keeps their computer
  and its files, because only directory removal deletes a computer. Should
  erasure delete their volume too?" — "yes."

**What was decided before, and missed.** No earlier record says in so many
words that a person's team messages go with them, but three say it
together: `2026-11-11-admin-sections.md` and
`2026-11-11-an-erased-persons-id-is-not-given-again.md` quote the spec
that removal "deletes their data", the latter with the owner's "users
should be isolated from each other securely"; and
`2026-11-11-a-members-shares-stay-theirs.md` quotes the owner on what a
member puts in a workspace: "The user should still own it." The
every-ledger erasure read the channel as the workspace's record instead and
kept the messages. That was the miss.

## Their team messages

- **Deleted for everyone, the way a sender deletes their own**
  (`2026-11-11-deleting-and-archiving-messages.md`). Before their content
  is removed, the erasure writes an `unsent` for each of their messages not
  yet deleted, in one write (`Kernel::unsend_all_of`, `team/forget.rs`).
  The text and every edit leave the log with the rest of their content and
  then the backups. The place stays, so a reply keeps its thread and the
  channel shows "This message was deleted" where it had replies. Its
  reactions, pin and edited mark go. The place names no one: the `unsent`
  is written with no sender and the state forgets the sender, because the
  state keeps nothing that names an erased person.
- **What the team agent posted on their request** (sender
  `agent:<them>`) goes with them too. It is their request's work, like the
  request's answer, which already went with their run.
- **Others' messages keep their own words**, a reply or a quote of theirs
  included. Their reactions, read marks, notification settings, archives
  and clears go. So do their requests' notes, the channel as their
  requests were given it, and their whispers.
- **Kept:** copies of the channel inside someone else's request, meaning
  the history the team agent was given for another member's request. It
  is that member's run record, as a sender's own delete already leaves it
  (`2026-11-11-deleting-and-archiving-messages.md`, "Not done"). Also kept,
  as before: the workspace's knowledge, projects, routines and files, which
  are handed on or are the workspace's.

**Whose team content is.** Team events name a `sender`, `requester`,
`owner` or `by`, not a `person`. So both owner rules gave their content no
owner, and `person_content_seqs` never found it. The rule now reads:

- A message and each edit of it are its sender's (`agent:<x>` is x's).
- A chat mark is the `person`'s it names.
- A note is its `by`'s.
- A request's channel copy is the requester's, and a whisper its owner's.
- Anything else is the `person` it names, or no one's.

It is written once in Rust (`team::owner`, applied as each event is
written) and once in SQL (`log::CONTENT_OWNERS`, for rows written before
and after a restore). The erasure also takes the events its ledger noted
for each message (`message_seqs`), so a row the SQL has not yet filled is
not missed. The SQL fills old rows when `magnus migrate` runs.

**Who may read is unchanged.** The owner column feeds two readers. One is
the per-person role (`recall`): it reads only seqs that the caller chose
from that person's own tasks, memory and trash, and never a team row. The
other is the erasure. Team messages are still read from the ledger, under
the workspace's membership. A backup does not carry the column, and a
restore refills it by the same SQL.

Test: `team::owner_tests` writes one team event of every content-bearing
kind, checks each owner as written, then clears the column, runs the SQL
and requires the same owner for every row in the log. Before, a message
was no one's. With the SQL half taken away, the test fails on the first
message. `team::forget_tests` erases mo through `erase_person` and checks
three things. Mo's message and edit are gone from the channel, the view
and the log. The place keeps sam's reply in its thread. Sam's words are
untouched. The state test (`forget_tests`) now seeds a message, an agent
post and an edit, and requires nothing of the erased person's in the team
ledger.

## Their computer

**The erasure deletes it and its volume** for every person, a config
person included (`Kernel::delete_computers_of`, `computer/one.rs`). It
uses the deletion that directory removal and the reconcile already use:
every grant revoked, the `Killed` that deletes the volume and forgets the
record, the computer's secrets taken from the vault, and computerd
destroying it and purging it within its window (`PURGE_WINDOW_DAYS`). It
runs after their runs stop and before their sessions and content go. If
it fails, the erasure stops there with a 502 and can be run again. A config
person is never gone to the reconcile, so a computer left behind would
stay. A person still on the server, which a config person is, gets a new,
empty computer at the next look, as anyone without one does. The erasure's
answer counts `computers_deleted`.

Test: `computer::one::tests::erasing_a_person_deletes_their_computer_and_its_volume`.
Mo, from the config, is erased. Their computer leaves the state, computerd
is asked to destroy it with its volume, sam and ana keep theirs, and the
next look gives mo a new one. Before, the erasure counted nothing and the
computer stayed.

This replaces two "what stays" lines of `2026-11-13-every-ledger-forgets.md`:
"Their computer's record" and "Their messages in a shared team channel".
