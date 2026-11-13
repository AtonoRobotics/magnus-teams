# One rule for message actions; the member view decides who may act

Step 3 of the server work named in 2026-11-13-team-items-are-chat-entries.md.

Before this, who could do what to a message was decided three times: in the
app for the personal chat (`personal-chat.js` describe), in the app for a team
channel (`team-conversation.js` describe, with its own guest and moderator
checks), and again in each team route. The app and a route could disagree.

Now each entry carries `may` (`crate::chat_entry::May`: edit, retry,
feedback, share, save, task, pin, delete), and the one set of message
controls (`message-controls.js`) draws from it alone. Copy, branch, export and
read aloud are anyone's who sees the entry. An entry with no `may` offers only
those.

- In the personal chat `May::personal` gives the person everything but pin:
  edit on their messages, retry on Magnus's turns, feedback on what Magnus
  said or did. This is what the app offered before.
- In a team channel `team::may::may` decides from the member view and the
  member's role, and every team message route (edit, delete, pin, share, save,
  retry) refuses through the same function (`team::may::check`), so the
  controls shown and the routes cannot disagree. The team's own rules stay
  here, each because a team needs it: what only its requester may see is not
  shared or saved for the team; a guest only shares; only who asked retries
  or judges the answer; who wrote a message edits it; who wrote it or whom
  the agent wrote for deletes it, and so do owners and admins outside a direct
  conversation.

The routes stay where they are: the personal chat's under `/v1/chat/{thread}`
and the team's under its workspace and channel, because a team message is
found through the member view of a workspace and a personal entry through
the person's own log. Bringing those under one path waits on one store
(step 4). Pinning now also refuses a message the member does not see (404).
