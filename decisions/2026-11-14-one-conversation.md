# One conversation, for one person or many

**Choice.** Above the two stores (kept, owner 2026-11-13: `2026-11-13-one-chat-store-not-yet.md`), the personal chat and a team channel are one thing on the server, `conversation::Conversation`: `entries`, `find`, `may`, `speaker_name`, `text`, `said_at`, `caps`. Two implementations:
- `conversation::personal::Personal { person, thread, entries }`: its entries are read by `Kernel::thread_entries`, from the turns and branch whose person is the caller, so to anyone else, the owner and an admin among them, the thread is not there.
- `conversation::channel::Channel { view: MemberView, channel, thread, now, names }`: what the member has is the member view's, and `may` is `team::may`.

Written once for both: an entry not had is a 404 (`no such message`), one `may` does not allow a 403 (`act`), and Export is one Markdown file (`export`). `team::may::check` and the personal chat's `find` are now these. The team chat's view is `Channel::entries`.

**Kept different on purpose**, each in its own routes and the app's per-source transport:
- Delete: personal to the trash, restorable; team gone for everyone ("It cannot be brought back"; nothing promises otherwise).
- Edit: personal resends and rewrites what follows; team edits in place, marked edited.
- Share: personal to anyone on the server through their inbox, within their rights and with the entry's taint; team quoted to a workspace member only.
- Branch: personal a server event; team quoted into a new chat in the app, unsent (inv8).
- Save: the person's Files, or the workspace's files.
- Reactions: personal a private verdict Magnus learns from; team public emoji.
No cross-scope action is added: no personal answer into a channel, no team message to the server's directory. That would be a new promise about whose words go where.

**Wire (fields added, none removed).** Both views carry `caps { several, reactions, pins, threads, whisper, typing }`: the personal thread all false; a channel all true but `whisper` for a guest. A team item's `text` comes without its `📎` lines (its files are `files`), and an answer or work carries `for`, its requester by name; `requester`, `task`, `task_id` and the rest stay for older apps. The personal thread's golden JSON gains only `caps`.

**App.** The view reads `caps().several` instead of guessing from `typeof conv.name`; a channel's reactions, threads and typing follow its caps, and a server that sends none is read as a channel with all of them. One adapter (`MessageControls.adapter`) runs both conversations' controls, each with a transport (`personalTransport`, `teamTransport`) of the same keys. `teamEntry` no longer strips lines or names requesters. Renamed: a whisper's Share `data-share` is `data-whisper-share`, `data-team-download` is `data-file-download`; `chat-view-check` holds every `data-*` the chat files use to a list.

**Also.** Team handlers that took `Member<at::X>` and returned `Reply` through `ok_or_return!` return `ApiResult` (invites, projects, routines, spend, upkeep, whisper, membership, search, own view, message actions); bodies and statuses unchanged. A team routine kept every firing in memory; it now keeps the latest `MAX_FIRINGS_KEPT` (32), and a change keeps only its last run (which may be open) and the change. The stop rule counts back at most that far, so `plan` stops a routine at `min(pause_after_failures, MAX_FIRINGS_KEPT)` in a row.

**Reason.** The rules for who has what and what a 404 or 403 says were written once per chat and once more per route; the app guessed a conversation's kind from its hooks and copied the controls' adapter. One trait with the differences named keeps the isolation model (users private, teams isolated) readable in one place.
