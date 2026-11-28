# Each person's agent has its own id

Step 1 of `2026-11-28-sharing-in-place`. The owner: "A user's agent and a
user should not share the same UID. The agent's UID should have the user's
name attached to it."

## Choice

- A person's agent is `agent:<person>` (`crates/magnus/src/agent_id.rs`),
  named "<Name>'s Magnus". The one shared id `agent` and the name "Magnus" as
  a member are gone. An answer, a run's work line, the computer's "using it"
  line, typing and notices all carry the agent of the person it acted for.
- In a shared chat a person asks their own agent with `@Magnus`: the mention
  stays one word, and the agent that answers is the asker's. The mention
  picker offers it as "Your Magnus".
- Who is working and who is typing are two lists: people type, agents work,
  each by its own name ("Mo is typing… · Samuel's Magnus is working…").
- "Talk to Magnus directly" is removed. A person's own chats are where they
  talk to their own Magnus; asking from the command palette outside a
  conversation opens a new chat of one's own. A chat with Magnus made before
  this (`Chat::Direct { agent: true }`) keeps its reader and is still
  answered, as that person asking their own agent.
- Dead code found on the way is removed: `channel_members`, `Why::Named`,
  `Why::Direct`, the direct-agent branch of `NewDirect`.

## Why

One shared id made every person's agent look like one member, so a shared
chat could not say whose Magnus answered or acted. The person stays the
requester on every act; the agent is who did it. The mention stays `@Magnus`
because a person only ever asks their own, and an apostrophe in a mention
would not survive typing.
