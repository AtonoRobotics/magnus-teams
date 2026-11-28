# Sharing in place

The owner, 2026-11-28: "A shared app should not copy. It should simply be
shared. The act of sharing converts it from personal information to shared.
If a chat is shared that loses its privacy. It should not automatically share
every past chat. It should be possible to share a chat or a project
individually." And: "A user's agent and a user should not share the same
UID."

Today sharing a project makes a shared copy (`team::promote`), so the person
sees the project twice. That copy is retired by this decision.

## Choice

1. **Everything has an owner and an audience; sharing widens the audience.**
   A personal project is a project with an audience of one. Sharing it adds
   people: the same project, its id, instructions and files. Nothing is
   copied. This needs one store for personal and shared projects and chats,
   which `2026-11-13-one-chat-store-not-yet` put off; it is done now.
2. **A chat has its own audience.** Inside a shared project a chat is the
   project's (its members see it) or only its owner's. Chats from before
   the project was shared stay only the owner's; a new chat in a shared
   project is the project's. One old chat can be shared into the project.
3. **Sharing a chat is not sharing its project.** A chat can be shared on
   its own; whoever receives only the chat sees the chat, not the project's
   files, instructions or other chats, and their Magnus in it cannot open
   the project's files. Someone a chat was shared with cannot share it
   further without its owner's approval: asking puts an approval in the
   owner's Approvals.
4. **Each person's own Magnus answers**, with their apps and rights, as now.
5. **An agent is its own principal.** A person's agent has its own id, not
   the person's, and carries the person's name ("Samuel's Magnus"). Every
   act records the agent and the person it acted for. An agent goes only
   where its person is: it cannot be invited without them, and leaves with
   them.
6. **Memory belongs to who said it.** What I say is my memory; what others
   say is theirs, never mine. A shared project's knowledge base keeps all of
   its members' memories and what is shared in it. Joining a shared chat or
   project shares one's data with it; the invite says so in one line: "What
   you say here is kept in the project's knowledge, for its members."
7. **Ownership and leaving.** A shared project has one owner, who may hand
   it to another member; deleting it needs the owner. Leaving loses access;
   what one said stays. A member's chats that are only theirs go back to
   their personal space with them. Unsharing narrows the audience from then
   on, and says that what was seen was seen.
8. **Sharing a chat shows what in it used private data.** Before a chat goes
   to new people, the answers in it that used the owner's private data are
   listed ("2 answers used your mail"), and the owner shares everything,
   leaves those out, or cancels. Nothing is shown when there is nothing to
   list.

## Order

1. The agent's own identity.
2. One store, audiences on projects and chats, sharing in place (the copy
   goes).
3. Sharing a chat on its own, with the re-share approval and the private
   answers list.
4. Memory by speaker, and the project knowledge base keeping its members'
   memories.

## Why

Copies made the person keep two of everything and lose track of which was
real. One rule (owner and audience) covers projects and chats alike, so
there is nothing else to learn. The checks that stay are the ones that stop
a real leak: a chat's history going to new people, and re-sharing without
its owner.
