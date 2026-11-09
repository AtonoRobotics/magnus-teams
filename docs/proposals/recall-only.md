# Recall only, behind per-person walls

Status: plan, for the owner's instructions of 2026-11-01. Ruling 9
said "approve per person data walls". Then: "Magnus should not hold
anyone's data in memory. It should be recall only." Each step is its
own pull request. The two questions at the end are for the owner.

## Why Magnus holds everything in memory today

At start, `Kernel::boot` reads every event in `public.events`, joined to
its content in `public.content`. From them it builds one `State`
(`state.rs`). That `State` holds every person's:

- tasks and their text;
- runs' replies;
- chat messages and answers;
- memory facts;
- library files and their search text;
- knowledge entries.

Every API answer and every boundary decision reads from it.

This was DEC 20, chosen on 2026-10-27 when the kernel was first built:
"`public.events` is the only source of truth. Every other table is a
derived projection … `serve` rebuilds it from the log on boot." It had
three reasons:

1. **Invariant 3.** All state is rebuilt from the log and its content.
   A projection held only in memory, rebuilt at every start, cannot
   become a second copy that drifts from the log.
2. **One view for decisions.** The boundary decides under one lock
   (taint, the Rule of Two, approvals, the kill switch). It reads one
   snapshot and appends to the log, with no query in between. DEC 191
   and DEC 294 later made sure memory never runs ahead of or behind the
   log.
3. **Simplicity.** There is one writer and a small organisation, so
   reads cost nothing.

Privacy was not weighed. The cost is this: whatever reads `State` can
see everyone. The walls between people are filters in code, and a wrong
filter shows one person another's data. A wall in the database would
not help, because the answer never touches the database.

## What changes

The log already separates each event into its **record** and its
**content**. This was deletion step 1 (`log::split`):

- The record is who, when, what kind, ids, status, taint labels, sizes
  and hashes.
- The content is what people and sources wrote (`events::CONTENT`).

That is the line:

- **Records stay in memory.** Decisions need them under the lock, and
  none of them is anything a person wrote. (See question A.)
- **Content is recalled, never kept.** Each request that needs content
  reads it from `public.content` as the person it is for. It reads
  inside a transaction that sets `magnus.person` and
  `SET LOCAL ROLE {role}_person`. Row-level security returns only that
  person's rows, so a wrong filter in code returns nothing instead of
  someone else's data. Content is held only while that request runs,
  then dropped.
- **Searches move into Postgres.** Memory search and library search
  become full-text queries under the same row policy. The knowledge
  index already works this way (`knowledge.rs`).
- **Model calls recall the run's history.** The agent loop reads the
  run's own content for each call, as the run's person, and holds it
  only for that call.

## The wall

- **Who a row belongs to.** `public.content` gains a `person` column,
  written with the content. It is the person whose task, chat, memory
  or file the event belongs to: the person of the event's run, or the
  event's own `person`. Content that is the organisation's, such as
  policy text or knowledge entries, is marked as such and read under
  the knowledge index's group rules.
- **Two roles.**
  - The kernel's role writes content and keeps the records' replay.
    It no longer reads content at start (`log::load` without the join).
  - A new `{role}_person` role, with no login, reads `public.content`
    only where `person = current_setting('magnus.person')`, under a
    forced row policy.
- **The admin.** By default people's data is the organisation's
  (ruling 14), so the admin's reads run as the person being read,
  through the same role, and are recorded. When the policy makes data
  private, the kernel refuses them before they reach the database.
- **Other readers** use the same role: exports, per-person backups, and
  any tool or person given SQL access.

## Steps

1. **Who owns each row, and the role.** Add `person` to
   `public.content`, written with every new event. `magnus migrate`
   fills it once for existing rows, from their events. `migrate` also
   creates `{role}_person` and its forced policy. Test: under the role,
   with `magnus.person` set to carol, no query returns alice's content,
   including a hand-written `SELECT` and a join through
   `public.events`.
2. **The recall path.** Add `Recall::for_person(person)`, which reads
   content by event seq or by item, through the role. Route the admin's
   data reads through it. Test: a reader forced to ask for the wrong
   person gets nothing.
3. **Chats and tasks** stop keeping text and replies in `State`. Views
   and the agent loop recall them.
4. **Memory facts** leave `State`. `memory_search` and the memory pass
   become queries.
5. **Library files** leave `State`: their bytes and their search text.
6. **Knowledge entries' text** leaves `State`. The index already holds
   it.
7. **Nothing is left.** `log::load` stops joining content at start.
   Test: after start, and after serving a chat, a task, a memory and a
   file, the kernel's `State` holds none of their content. The test
   seeds unique marker strings and checks that none appears in `State`
   serialised.

Each of steps 3 to 6 has its test: the views and tools give the same
answers as before, and the content is absent from `State`. The
invariant tests drive the appliance from outside, so they should pass
unchanged. Invariant 3 still holds: records are rebuilt from the log,
and content is read from the content store.

## What it costs

Each view becomes queries. Views refresh every 2 seconds, and Postgres
runs on the same machine, so the cost is small but real. Step 3 measures
the Tasks and chat views before and after.

## Questions for the owner

- **A. Records.** They hold no one's words, but they do say who did
  what and when (for example, alice ran a task at 09:12, using mail).
  - **Recommended:** keep records in memory, because decisions need
    them under one lock.
  - **Alternative:** move them into projection tables in Postgres,
    which DEC 20 permits. Each decision then runs inside a database
    transaction instead of the lock. This is slower and a larger
    change.
- **B. The reply being written.** The live text of an answer in
  progress (`kernel::Live`) is in memory while the model writes it, so
  a view that opens mid-reply sees it. It is dropped when the reply is
  logged.
  - **Recommended:** keep it.
  - **Alternative:** write it to the content store as it streams.

## Not in this plan

- Encrypting each person's content under their own key (the
  authentication design's per-user keys) is separate. The wall limits
  who may read rows; keys limit what a copy of the disk reveals.
- Walls on `public.events` itself. It holds only records, which the
  kernel needs whole.
