# One chat view: a team channel is the personal chat with several people

**Choice.** There is one chat view in the app (`desktop/ui/chat-view.js`) and one composer (`desktop/ui/chat-composer.js`). The personal chat (`personal-chat.js`), a team channel and a team thread (`team-conversation.js`) are each a conversation given to that view: where its items come from, what a message is to its controls, and what each control does there. The view draws the layout (head, list, Jump to latest, status line, composer slot), every message and answer, the work rows, the controls (through `MessageControls`), the edit box, files, scrolling and reading place, motion and the live status lines. What several people add is drawn by the same view when a conversation names its people (`name`, `me`), and is absent when it does not: who sent each message with their face, another person's message on the left, mentions marked and offered while typing, reactions, replies and threads, the requester named on an answer and on a work row, the note on an answer only its requester sees, who is typing, a catch-up and one's own Magnus whispering. A thread opens in the same view beside its channel. `team-chat.js` keeps only a channel's state, its head's tools (faces, how Magnus takes part, notifications, pins, archive) and refreshing. The team's own message renderer, composer, mention box, file tray, optimistic sends and its `.tm` styles are deleted.

**Reason.** The owner: "magnus team chat interface still doesn't match the single user chat interface. These should not be two novel systems. Teams is a multi user version of the single chat interface", and then "Not the same system". Shared parts would still have left two systems that drift; one view with two conversations cannot.

## What differed, and what became of it

| Where | Personal chat | Team channel before | Now |
|---|---|---|---|
| Frame | `section.conv`: head, thread list, Jump to latest, composer | `.team-head`, `.team-scroll`, typing line, whispers box, `.team-composer` | the personal chat's frame in both; typing and whispers inside it |
| Head | project link over the title, tools on the right | title, project link under it, pill tools | the personal chat's head; the channel's tools in its row |
| A person's message | right-hand bubble, "You · time" | Slack-style row: avatar, bold name, time, "(edited)" | the personal bubble; others' on the left, with name and face; "· edited" |
| Magnus's answer | left, "Magnus · model · time", markdown | row with "team agent" chip and "for X" | the personal answer; "· for X" |
| Work | "From this request" task rows with icon, status pill, live status line, Stop | `.tm-work` box with spinner and "Open its thread" link | the personal task row (`ChatView.workRow`), requester named, row opens its thread, live line through `statusLineSlot`; no Stop (no team route stops a request) |
| Files | uploaded on attach, chips with Retry/Remove; previews with Download | tray filled on drop, uploaded on send; `.tm-file` chips | the personal composer's chips and upload-on-attach; the personal `.fprev`/`.fchip` preview with Download |
| Composer | `form.say`, auto-growing box, model picker, attach, send arrow; Enter sends; text required; text stays until the server confirms | `.team-composer` textarea, attach button, whisper checkbox, Send; files without text allowed | the personal composer; the bar holds "Only my Magnus sees this" where the personal one holds the model picker; mentions offered in a list above the box |
| Sending | box kept, Send disabled until the server answers; error toast keeps the words | message shown at once as "Sending…", "Not sent · Retry" | the personal behaviour |
| Edit | box in the bubble, Send / Cancel | box in the row, Save / Cancel | the personal box; Send saves the edit (the 📎 lines are kept) |
| Reactions, pins, delete | Feedback reactions in the dialog; delete waits for the server | shown at once, synced behind | wait for the server, then redraw, as the personal chat does |
| Drafts | per conversation for the session (MessageDrafts) | localStorage per channel and thread | MessageDrafts in both, and a channel's also kept in storage across restarts, as before |
| Scrolling | reading anchors, Jump to latest | scroll to bottom when near it | reading anchors and Jump to latest in both |
| Empty state | `.hello` | `.empty` | `.hello` in both |
| Status | "Working on an answer…" turn, live status lines | "X is typing…" line | the status line under the list carries who is typing; work rows carry live status |
| Drop files | not offered | drop on the channel | drop on any conversation attaches to the next message |

## What still differs, and why

Only what several people need: the sender's name and face, mentions, reactions and Reply in thread (the team's own controls first in the row), the thread pane, the requester on work and answers, the private-answer note, whispers, typing, the catch-up chip and the channel's head tools. The personal chat keeps what only one person's conversation has: the model picker, its decision cards (suggestions, grants, schedules, connector offers, sub-agents, results with their deliverables) and Stop on a task. Read aloud of a team answer still reads it as written (decisions/2026-11-13-one-message-controls-module.md).

## The server still has two models

The personal chat's items are entries of `/v1/chat/{thread}` (kinds `message`, `answer`, `turn`, `tasks`, `result`, cards); a team channel's are `TeamItem`s of `/v1/workspaces/{ws}/channels/{c}/view` (`crates/magnus/src/team/items.rs`), read into the view's kinds by `teamEntry` in the app. Recommendation: have the team view send the chat entry shape itself (the same `kind`s, `task_id`, `files`, and a `tasks` work row), so the app reads one wire type, and keep in the team only what it genuinely needs: membership and channel visibility (the member view), a private answer shown only to its requester, team words never counted as the person's own context or taint (inv8), and per-workspace isolation.
