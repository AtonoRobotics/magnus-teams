# Chat entries stay typed to the edge

Before this, the personal thread (`entries_with`) built typed entries,
turned them into `serde_json::Value` and then changed JSON fields. Every
consumer looked fields up in that JSON: the message actions, reactions,
search, the thread's taint, the next turn's transcript, a branch's copy.
The list of Magnus's kinds was written out twice (`reactions::MAGNUS` and
`May::personal`).

**The choice.** `chat_thread::Entry { id, kind: Kind, at, task_id, may,
reaction, body }` is the thread entry until it leaves the server. The body
is one of `Said(ChatEntry)` (message, opener, answer, started tasks: the
type both chats share), `Turn`, `Result(TaskResult)`, `SubAgents`,
`Card(Map)` or `Copied(Value)`. `Kind` travels as today's strings. What the
server reads from an entry is a method: `text`, `speaker`, `markdown`,
`file_name`, `task_ids`, `from_run`, `view`, `spoken`, `said_text` (search),
`transcript_line`, `is_magnus`. Serialization (`Entry::value`, and
`Serialize` through it) happens only where an entry leaves: the thread
route, the admin's view of a person's chat, a branch's `ThreadBranched`
copy and a shared item's details. `Kind::is_magnus` is the one list of
Magnus's kinds: what takes a reaction and what the person may judge.

**Two bodies stay JSON, each for a reason.**
- A card's fields are the model's (`suggest`, `offer_app`). The server adds
  its answer to them (state, schedule, grant) and they stay a map.
- A branch stores its entries in `ThreadBranched` as they showed when it
  was made. An event kind is never edited, so a copy replays as stored,
  marked `copied: true`, and is never typed again. A kind this server no
  longer makes reads as `Kind::Other`.
Both are read by one reader (`entry::map_text`, `entry::map_line`), the
legacy reader for an entry in JSON form.

**Wire compatibility.** A golden test (`chat_thread/golden_tests.rs`) was
committed before any of this. It pins the exact JSON of `GET
/v1/chat/{thread}` for a fixture thread with every kind of entry, a branch
made now and one stored in an older form, and it passes unchanged at every
commit after. `ChatOrigin.conversation` keeps its stored `{role, text}`
form. One thing differs at the margin: a stored copy with an id but no
`kind` now gets the actions of an unknown kind (share, save, delete)
instead of none. No branch was ever stored that way, because copies are
made from entries that always have a kind.

**Where the rest went.** `chat_work.rs` is gone. Its sub-agents entry
moved into `chat_thread.rs` with the other entries. Its two state queries
(`chat_turn_above`, `in_chat`) moved to `sub_agents.rs` beside the other
sub-agent queries. The rule for where a tool may be used (`placement`,
`record_placement`, `is_chat_only`) moved to `run/admission/placement.rs`,
next to the admission that applies it, once the tool registry had merged.
`chat_entry.rs` stays its own module because the team chat builds
`ChatEntry` too.

Not in this change: the "one conversation" work (capabilities on the wire,
a conversation trait, the UI adapter). It follows the route rewrite.
