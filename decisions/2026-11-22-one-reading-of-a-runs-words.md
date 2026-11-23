# One reading of a run's words, each part by who wrote it

**Choice.** `crates/magnus/src/run_words.rs` is the one place that works out
the words of the request a run acts on. `Kernel::run_words(run)` returns a
`RunWords`: each part tagged with its `Source` (`Person`, `Model`,
`Schedule`, `Other`, `Server`), and marked as the request itself or a chat
turn's earlier thread. Every caller asks by name for the sources it trusts:

| Caller | Trusts | Reads |
|---|---|---|
| `ensure_envelope` (`envelope/resolve.rs`) | Person, Schedule | the request |
| `web_sources` (`web_reads.rs`) | Person, Schedule | the request |
| Auto Review (`auto_review.rs`) | Person, Schedule | the request |
| the send reviewer (`run/reviewer.rs`) | Person, Schedule, Other (each said as what it is) | the request |
| `RunWords::named_by_person` (was `boundary/call.rs`) | Person | the request |
| commitments' `said_in` | Person, and Model for Magnus's own | the request and a turn's thread |
| the memory pass (`run/memory.rs`) | Person | the task's own text |
| `task_from_turn` (`chat.rs`) | Person | the task's own text |

How a task's words are tagged: the words are always the requester's (the
top of a sub-agent's parents), never a sub-agent's goal. A chat task's text
is `Model` (the turn that started it wrote it); its carried conversation's
`person` lines are `Person` and `you` lines `Model`. A chat turn's text is
`Person`, its earlier thread is said but not the request (write authority
resets per turn). A schedule's text is `Schedule`. A task someone else made
for the person (`by_other`) is `Other`, its conversation's person lines
too. An opener's, an outcome turn's and a review's text is `Server`. A task
result's or card's line of a conversation is nobody's words and is left out.
`Kernel::asked`, `Kernel::own_asked` and `Kernel::named_by_person` are gone.

Two bugs are fixed with it:

1. The envelope read `own_asked`, which for a chat task returned the goal the
   model wrote, so a destination the model put in the goal was allowed. The
   envelope now compiles only from the person's words and a schedule's.
2. Auto Review built its cache key before reading the request and left the
   request out of it, so a cached Allow carried to another task's request for
   up to `CACHE_FOR` (thirty minutes). The request is read first and is part
   of the key.

The same reading fixes the same hole where it was repeated: `web_sources`,
Auto Review's `what_the_person_asked` and the send reviewer also took a chat
task's goal (and the reviewer a sub-agent's goal) as the person's words, and
the memory pass filed a chat task's goal, a schedule's text and a teammate's
lane as "the person said". A review's text (the server's instruction over
the person's state, which quotes ended tasks' outcomes) counted as the
person's words everywhere; it now counts nowhere that authorizes.

**Reason.** `decisions/2026-11-20-compartmentalized-security-and-envelopes.md`
says an envelope is "compiled lazily on the first outbound write attempt from
**only the user's own messages** (never tool results or assistant turns)".
A chat task's goal is an assistant turn's words. Eight callers each had their
own reading and they disagreed; one reading, asked for by source, makes the
disagreement visible in each caller's trust list instead of in its control
flow. Schedules stay trusted by the envelope, Auto Review and `web_sources`
because spec rev 140 says "a schedule the person set up ... authorizes the
work it describes" and the 2026-11-20 decision scopes envelopes to schedules;
they stay out of `named_by_person` and commitments, which want what the
person said, not a standing instruction. `Server` is a fifth source beyond
the four the problem named: an opener's, an outcome's and a review's text is
neither the person's nor the model's, and calling it either would let it
authorize or be quoted as Magnus's words.

**Not changed.** A team routine's run (`actor = "poll"`) keeps its text as
the person's, as before: team code is being changed by other work. A shared
team thread's other members' messages reach a conversation as `person` lines
(`chat_thread/entry.rs::transcript_line`); telling them apart needs the
entry's author there, which is outside this change.
