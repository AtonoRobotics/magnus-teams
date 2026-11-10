# The team workspace works like a team chat, with the agent as a member

The team UI is rebuilt to the spec's "Workspace experience": three panes (rail, conversation, project panel), threads, mentions, reactions, edits, pins, drafts, typing and presence, per-channel notifications, an activity view, projects with a board, files and knowledge, one search, a command menu on Ctrl or Cmd+K, and changes shown before the server confirms them.

Choices made, with their reasons:

- No button asks the team agent. It answers when @mentioned or named as a word; "my Magnus" and "your Magnus" are personal agents and do not count. In a direct conversation with it, every message is to it. A guest's message never starts it, since a guest reads and chats only.
- Each channel has a setting: mentions only, or join when it can help. A new channel is mentions only, because joining unasked spends money and posts where no one asked; a member turns it on. "When it can help" means a member catching up ("catch me up", "what did I miss") or a question put to the room ("can someone", "does anyone"), and a thread whose last message is a member's question left unanswered for 24 hours, followed up once, for that member. Follow-ups look back 7 days so turning the setting on does not wake old history.
- Long work gets its own thread. Work is long once it has run 20 seconds or taken 3 steps. Its live card sits on the message that asked, with the requester named, and its progress posts and answer go in the thread on that message. A quick answer stays in the channel. Asked inside a thread, everything stays in that thread.
- The agent is in every channel from the start, including direct messages, and every new workspace starts with a #general channel, so there is somewhere to talk at once.
- A project is made by typing its name; it gets its own channel and its maker as owner and first member. The agent can make a project or channel too (`team_create`), owned by the member it works for.
- Messaging changes, the board, invites and channel settings travel as one new team event shape, `chat`, listed as content, so they leave the log and the backups with the workspace. Typing and presence are moments, kept in memory only.
- Invites: by email, someone the server already knows joins at once; otherwise an invite code is made, and its link is shown to copy. The server sends no invite email itself. A link works for 7 days; a code bound to an address is used once.
- Notifications are per channel and per person: all, mentions or off. The desktop app notifies while a workspace is open.

Not done here: per-workspace and per-channel spend limits in the UI, and the agent flagging something a member needs on its own.
