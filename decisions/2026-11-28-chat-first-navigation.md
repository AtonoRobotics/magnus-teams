# The app is navigated as a chat agent is

The owner, 2026-11-28: Projects are "bureaucratic security theater"; "review
the UI/UX as well. Simplify the navigation and steps to be more like other
modern chat agents."

## What a modern chat agent would not have (the review)

1. Seven primary items above the chats: New conversation, Inbox / Approvals,
   Projects, Briefing & Routines, Explore, Apps & Skills, Trash.
2. Each project in the sidebar a disclosure with its chats nested under it, so
   a chat showed in two places or only inside its project.
3. Shared projects drawn apart from personal ones, with a "Shared" badge.
4. A People section listing every member of every shared project, and a
   "Start or join a shared project…" row.
5. Conversations split into Today / Previous, apart from shared chats.
6. Quick Jump at the foot, an Appearance button beside Settings.
7. A project page's "Ask Magnus" only filled the next page's composer: the
   person pressed send a second time.
8. A "New conversation" button beside the composer that did the same.
9. A shared project's composer hidden when it had no chat.
10. "New shared chat name…": a chat named before anyone spoke.
11. Add people opened a dialog that copied the project: a project picker, a
    name, a document checkbox each, an instructions checkbox, an invite-link
    checkbox, "Create shared project".
12. The conversation Share dialog's boundary note and its "What crosses into
    the shared conversation" fieldset with radio buttons.
13. Reassurance: "(nothing runs until you send)", "nothing runs until you ask
    Magnus", "Magnus waits until you ask", "Start one when you're ready",
    "Your private project stays private…", "Personal spaces are untouched",
    "Your original project stayed private.", "works in private and shares
    scrubbed outputs", "Personal conversations and computers are never
    touched", "No one else sees these", "Nothing starts until you say so".
14. Two project pages, personal and shared, built apart: the shared one with
    a "Shared" badge, a people-count button and People said again below.
15. Instructions labelled "Optional" or "Active" beside their own text.
16. A private answer that stopped showed "Working on it…" forever.

## The choice

- **Sidebar.** New chat; Search (⌘K); Projects, personal and shared in one list
  by name, a shared one carrying a small people icon. Each project opens a
  list of its own chats under it by its chevron, newest first, at most six
  and then "All n chats" to its page; the project of the open chat opens by
  itself, and one the person opens stays open between visits. The
  difference between a personal and a shared project is inside it, not in
  the navigation (owner, 2026-11-28). Recents is the chats in no project and
  the direct ones with people, newest first: a chat is listed in one place,
  under its project or in Recents, not both, as a chat agent's projects do.
  A shared chat is ordered by its latest message, the server's `last_at`.
  Approvals (with its count), Briefing & Routines, Explore, Apps
  and Trash are one row of icons at the foot, above the person; Settings is
  the person's initial or gear. Nothing is deleted: each page is where it was.
  The open sidebar has one control that collapses it; the rail's sidebar
  button shows on the rail only.
  People are reached from a shared project's page, where each member is a
  link to a direct chat.
- **A project's composer sends.** In a personal project, the message goes to
  a new chat in the project through the chat's own composer (the model,
  autonomy and plan choices apply as to any first message). In a shared
  project the composer is always there for members: the message starts a
  chat named by its first words; with no chat yet, or when one cannot be
  made, it goes to the project's general chat, which the server makes. No
  one names a chat before talking.
- **Add people is one step.** A people picker (the server's directory, and an
  email box for someone new) with one button, Share. The server copies the
  project's name, instructions and files into the new shared project and
  adds the people (`POST /v1/projects/{id}/share` with `{ "people": [...] }`,
  answering `{ "workspace": "..." }`); an email is then invited to it, and
  the shared project opens. A link no email carried is shown to send.
- **Share a conversation** keeps its choices (which shared project, the chat's
  name, people, the summary, or chosen messages instead) without the boundary
  note or the explanation of what crosses.
- **One project home.** Personal and shared projects are one page: the name,
  editable in place (a shared one by any member, as the server allows, `PATCH /v1/workspaces/{ws}`
  with `{ "name": "..." }`), the composer, Chats, Files and Instructions;
  Tasks and Routines when a personal one has them; People only on a shared
  one.
- **Reassurance goes**, as decisions/2026-11-28-ui-without-ceremony.md says of
  ceremony. "Only you can see this" stays, as a tag.
- **A private answer that stopped** says why (`held_because`), and can be put
  away.

## Why

The owner asked for the shape every chat agent shares, and the spec asks for
none of the steps that went: they were ceremony (a second Send, a name before
a message, a count beside the list it counts) or reassurance in place of
information. Approvals keep their count on their icon, so a decision waiting
is still seen at a glance. Team chat stays the personal chat with several
people (decisions/2026-11-13-one-chat-view.md): one project
page, with only People added.

`desktop/tests/chat-first-nav-check.mjs` and `projects-flow-check.mjs` check
each of these.
