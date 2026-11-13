# One message controls module for the personal chat and the team chat

**Choice.** `desktop/ui/message-controls.js` (`MessageControls`) decides which controls a message has, draws them, and runs the one chosen; the personal chat (`app.js`) and a team channel (`team-chat.js`) each give it only what the message is and an adapter of what each action does there. Both chats had their own copy of the list, the icon row, the menu, the share row and the Feedback dialog, and the copies had begun to differ.

- `controls(message, caps)` is pure: who wrote it (`mine`, `agent`), whether it can be read aloud, whether the viewer asked for it (`retry`, `feedback`), whether only its requester may see it (`private`: no Share, no Save to Files), whether the viewer may delete it, and the viewer's caps (a guest: no Save to Files, Make a task or Pin; the team has Pin). It is held to a table of cases in `desktop/tests/message-controls-check.mjs`, which the fast tier runs.
- `run(act, adapter)` calls the adapter's method of the action's name (`share-send` is `shareSend`), and opens the one Feedback dialog from what the adapter's `feedback` gives. The dialog gives focus back to the message's menu button when it closes, in both chats; the team's did not.
- The team's adapter sends Share, Save to Files and Retry to the server's routes (decisions/2026-11-13-team-message-actions-on-the-server.md).

**Behaviour kept, and what changed.** Each chat offers what it offered before, with two changes that were copies drifting apart:
- The team agent's answer's Copy is "Copy response", as the personal chat's answers are; a person's message stays "Copy message".
- The team's Retry asks the request's own words through the server, not `@Magnus` and the work card's 80-character title.

**Read aloud stays the message as written in the team chat.** The personal chat asks its server for a spoken script of an answer (`/v1/chat/{thread}/entries/{entry}/spoken`); there is no team route that writes one, and adding one would have the model read a team answer for a single listener, a new run over team words that the team chat has never made. Read aloud reads the team answer's words as they stand, as before.

**Reason.** One list of who may do what with a message, read the same way by both chats and tested as a table, cannot drift again; each surface keeps only the parts that are really its own (its routes, the team's reactions and threads).
