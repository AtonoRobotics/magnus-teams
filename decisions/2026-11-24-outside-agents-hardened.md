# What a hostile outside agent cannot make the server do

**Choice.** An independent review of the outside-agents work found these
gaps. Each is closed, with a test that fails without the fix.

- **Every body is read with a cap.** A card or key set is read up to
  `MAX_CARD_BYTES`, a JSON-RPC answer up to `MAX_ANSWER_BYTES` and a token URL's
  answer up to `MAX_TOKEN_ANSWER_BYTES`. Each is read piece by piece and
  refused once it passes its cap (`client::read_capped`), never read whole and
  checked afterwards. An answer that is a message rather than a task is
  bounded like a task's words.
- **The objective is quoted safely.** The run's objective quotes the agent
  safely. The card's name is kept as one line of at most
  `MAX_AGENT_NAME_CHARS`. The name and the request are escaped (`&`, `<`, `>`,
  `"`), so nothing in the request can close `<outside_agent_request>`, and
  nothing in the name can end its attribute.
- **A project request's GetTask carries only what a person wrote:** posts by
  the project's members. An agent's or lane's post (`agent:..`) in the
  thread was released into the room, not to the outside agent. It is
  withheld rather than sent through a second release. A person who wants
  the agent to have it can post it themselves.
- **Admission is read again at each request and each GetTask.** For a
  shared project, the member who added the agent must still be a Team Admin
  of a project that still exists. For a person, it must still be the agent's
  own member. Otherwise the request is refused (`NOT_ADMITTED`) and nothing is
  posted or run. The stored Cedar policy alone no longer decides.
- **The ids a request or an admission names are checked before Cedar.**
  An id must be 1 to `MAX_TARGET_ID_CHARS` characters, with no control
  character, quote or backslash. An id that fails this check is
  `INVALID_PARAMS` (or a 400), never a 500.
- **A card's key set must be on its own origin:** scheme, host and port,
  not the host alone.
- **An address an agent names has no query.** A query is refused, which is
  what `checked_url` has always said.
- **One key per rotation period.** Making a period's card signing key is
  atomic, so two first requests at once cannot store two seeds for one
  period.

**Reason.** An outside agent is untrusted input from beyond the server
(invariant 4). What it sends must not cost unbounded memory, steer the
planner past its quote, read agent-made output that no one released to it,
or outlive the authority that admitted it (invariants 1 and 6). Withholding
agent-authored project posts is the narrower of the two options the review
allowed. It adds no new path out of the room.
