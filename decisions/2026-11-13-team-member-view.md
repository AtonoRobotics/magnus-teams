# What a member sees of a workspace is decided in one place

**Choice.** `team/member_view.rs` holds what one member sees of a workspace: the channels they see (a direct conversation's people, a project's members, else everyone), the messages they still have (nothing deleted for everyone, nothing from before they deleted the conversation for themselves), the agent's work and answers (an answer built from data only its requester may see reads in full to them alone), the files they may read, and one message or answer looked up by id. Every route that shows or acts on these asks it: the workspace, the left rail, the channel view, the plain message list, export, edit, delete, activity, search, notices, and the channel checks of whispers, upkeep and spend. The routes only shape what it returns. The channel's items travel as one set of types (`team/items.rs`).

It reads the ledger and each request's outcome (a small `Outcome`, from the kernel's state or from a table in a test), so its rules are tested with no server and no database (`team/member_view_tests.rs`).

**Fixed on the way.** The copies of these rules disagreed:
- `GET .../channels/{channel}/messages` listed an agent answer after it was deleted for everyone; the channel view hid it. It now lists what the view does, without the live cards, and in the view's item shape.
- Notices told a person of an answer from before they deleted the conversation for themselves, and of an answer deleted for everyone.
- Editing a message deleted for everyone found it and would have put words back into the log. A deleted message is no longer found to edit.
- Search left out what was cleared but read deleted messages by their (emptied) text; it now reads the view's messages.

The place a deleted answer leaves now names the thread it was shown in, as its replies hang there.

**Reason.** Each route rebuilt "what a member sees" from the same primitives, and each new route was a new chance to miss one, as the messages route did. One module makes the rules one decision, and the next route gets them by asking it.
