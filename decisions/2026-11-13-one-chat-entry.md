# One chat entry type for both chats; team work is a personal task row

Steps 1 and 2 of the server work named in 2026-11-13-team-items-are-chat-entries.md.

**One type.** `crate::chat_entry::ChatEntry` is the one entry the chat view
reads. The personal chat (`chat.rs`) builds its opener, message, answer and
`tasks` entries with it, and the team chat's items (`team::items`) are the
same type (`TeamItem` is now an alias). A field a kind lacks is left out. Team
files are file entries (`{id, name}`, the workspace file's name as its id), as
a personal message's are (`{id, name, media_type, size}`). The kinds only the
personal chat has (app and suggestion cards, results, sub-agents, a turn's
progress) are still JSON where they are made: they carry many kind-specific
fields, and typing them changes no shared behaviour. They are next if a
second chat ever needs them.

What changes on the wire for the personal chat: a field that was `null` is
now absent (`view`, `spoken`, `verdict`, an opener's `app`), and an answer
with no words has `text: ""`. Rust's `value["x"]` and the app's `e.x == null`
read both the same.

**Team work as the personal `tasks` entry.** A request's work is a
`TaskRow`, the personal `tasks` entry's row (`task_id`, `title`, `text`,
`status`, `waiting_on`, `waiting_item`, `status_line`), with a team request's
own fields (`requester`, `steps`, `doing`, `long`, `thread`, `started`). Work
no message carries is a `tasks` entry with its one row; work a message asked
for rides on that message as its row. The app draws both with the chat
view's work row, as it draws a personal task.

The team keeps its own here only where teams need it: `requester` (the card
names who asked, and in a team thread only the asker sees the card), and
`thread` (long work gets its own thread, visible to everyone).
