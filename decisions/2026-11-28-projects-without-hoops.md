# Projects without hoops

The owner, 2026-11-28: "magnus project is bureaucratic security theater.
It's impossible to do anything without jumping through many hoops." Then:
"Review Magnus Projects".

The review found that most of the friction was in shared projects, and that
much of it came from the retired team agent: rules and settings that nothing
reads any more, or that start every run as if it held the person's secrets.
The owner chose eight changes. The server half is here; the app half is in
`2026-11-28-chat-first-navigation.md`.

## Choice

1. **An answer in a shared chat posts.** A room ask's run still starts
   private for the room's words, so nothing it files from them is public.
   But those words going back to the same shared project leak nothing. So a
   post to the room or its chats is held after an outside read only if a tool
   brought the run private data (`Run::read_private`, set from the tool
   result's label in `state::work`), or the run started with a named private
   source such as an upload (`lane_release::holds_private_for`). An answer
   from the web posts to the chat. One built from the person's mail after an
   outside read is still held. A reply to an outside agent keeps the full
   rule: any private mark holds it. The shared project's own knowledge and
   files are private too, but named as their source (`team::project_source`,
   read from the result's `private_source`), so an answer from them and the
   web posts back to that project. Another project's knowledge, or a reply
   to an outside agent, is still held.
2. **No answer is dropped without a word.** The reply watcher no longer stops
   after 15 minutes. After that it polls every 5 seconds until the run ends,
   so an answer that waited on an approval still posts. A run that failed or
   was stopped says so on the asker's private answer (`held_because`).
3. **Magnus reads the shared project.** A person's agent asked in a shared
   chat gets the shared project's own knowledge base, not only a
   sub-project's that the chat happens to be tied to. It is also told that
   `team_files` and `team_read_file` open the project's files.
4. **A shared project's apps work.** When a person asks in a shared chat and
   has no sign-in of their own to an app, the call goes through the shared
   project's sign-in. This needs a project that granted that app to their
   role and is signed in to it (`Kernel::app_owner`). Before, only the retired
   team agent read the grants, so granting an app did nothing. A guest still
   gets none.
5. **Share a project in one call.** `POST /v1/projects/{id}/share` with
   `{ "people": [...] }` makes a shared project with the project's name,
   instructions and files and its first chat, adds the people as members,
   and answers `{ "workspace": id }`. The personal project and its
   conversations stay in the person's own store
   (`2026-11-13-one-chat-store-not-yet`).
6. **Members rename a shared project.** `PATCH /v1/workspaces/{ws}` with
   `{ "name" }`, at the member role, as instructions are. This adds the event
   kind `Change::Renamed`.
7. **Members fix their own facts.** The member whose words alone an entry
   holds may correct, retire or delete it. Before, only an admin could.
8. **The monitor is asked once per reply.** A reply's calls are checked at
   once; each now waits on the one ask in flight instead of making its own
   (`safety::watching`), as invariant 4's monitor test asks.

## Why

Each one made a person repeat themselves, wait on someone else, or lose
their work with no word. None protected anything an invariant asks for.
The checks that do stay: the scrubber, the taint hold on private data after
an outside read, the outgoing classifier, Auto Review on posts, and the
guest limits.
