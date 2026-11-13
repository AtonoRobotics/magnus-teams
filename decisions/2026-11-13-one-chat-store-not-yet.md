# One store for both chats: what it takes, and why it is not done in this change

Step 4 of the server work named in 2026-11-13-team-items-are-chat-entries.md
asked for shared storage, with a reader for the old team events.

What each chat stores today:
- A personal message is a chat turn: a task (`task_created` with `chat`
  origin) and its run. The words are recalled from the person's own store
  (`recall::Words`), the answer is the run's summary, and the thread is the
  set of turns with that thread id. The entry list is derived from them.
- A team message is a `Team(Change::Message)` event in the workspace ledger,
  with edits, reactions, pins and deletions as `Team(Change::Chat)` events.
  Only a request to the agent becomes a task and run, and the answer is that
  run's summary, as in the personal chat.

So the answer side is already one store (tasks and runs). What differs is
the person's words: a team member's words are a shared ledger event any
member reads; a person's words are kept in their own store, never shared.

Moving both onto one event (a new `ConversationMessage { conversation,
sender, text, … }`, with readers that translate old `task_created` chat turns
and old team `Change::Message` into it) touches the source of every personal
thread, recall, the trash, branches and the personal-context and taint rules
the invariants hold (inv8: team words never count as the person's own
context). It cannot be done as a refactor: every personal thread on the
server would be read through the new reader from its first event, and an
error there loses or leaks a person's words.

Recommendation: keep the two stores. The person's words stay theirs, in their
own store; a team's words stay in the workspace ledger, where the member view
reads them. Everything above the store is now one: the entry type
(2026-11-13-one-chat-entry.md), task rows, the message-action rule
(2026-11-13-one-message-actions.md), the chat view and the composer. If the
owner still wants one store, it should be its own change with a replay test
over a copy of the production log before and after, and an owner review of
the inv8 tests it touches.
