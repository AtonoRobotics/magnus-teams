# Deletion: content leaves the log, so it can be deleted

Status: proposal (owner, 2026-10-29). The owner's decisions are below.
Invariant 3's new wording needs the owner's approval before any code.

## Why

Everything Magnus stores is in the append-only event log, and nothing can be
removed. Chat "Delete" only hides an entry (`ChatHidden`), and a deletion
request is answered by hand. The owner's points: the disk fills, and junk and
mistakes stay forever. Conversations, tasks, files, memory and knowledge must
all be deletable.

On strix-alpha after two days the log is 21 MB in 8,357 events (Postgres
sizes, compressed). Tool results are 13 MB of it: fetched pages, screenshots
and shell output. Delivered and uploaded files are stored in the log as
base64, up to 25 MB each.

Encrypting each item with its own key and destroying the key on delete makes
content unreadable, but the bytes stay on disk, so it does not meet the
owner's first point. Content has to leave the log.

## The owner's decisions

- Content moves out of the log into a store where it can be deleted. The log
  keeps the record.
- Raw tool results expire on their own once their task has ended.
- Content already in the log is moved out once.
- Deleting an item deletes only that item, not what came from it (memories
  it wrote, files it delivered, tasks it started, knowledge it filed).
- A 30-day trash for conversations and other useful context (the list below
  is a proposal).

## Design

### The record and the content

- **The log (unchanged rules: insert only, never rewritten by the kernel)**
  keeps what happened: event kinds, ids, who, when, which policy decided,
  taint, costs, sizes, and the SHA-256 of each piece of content.
- **The content store** is a new Postgres table, `content`. Each row holds an
  id, the person it belongs to, the item it belongs to (a conversation, task,
  file, memory or knowledge entry), its kind, its bytes, and the dates it was
  created, trashed and expires. The kernel's role may select, insert, update
  the trash and expiry dates, and delete rows here. On `events` it keeps only
  select and insert.
- Content means everything a person or a source wrote:
  - message and task text
  - model replies
  - tool call inputs and results
  - contracts
  - spoken scripts
  - files
  - memory facts
  - knowledge entries

  Events refer to it by id and hash.
- State is rebuilt from the log as now. Content is read from the store when
  needed; content that is gone reads as "deleted" or "expired".

### Trash (proposed list)

- **Trash, restorable for 30 days:** what people make or curate.
  - conversations, and single messages in them
  - tasks, with their outcome and delivered files
  - library files and projects
  - memories
  - knowledge entries

  Chat "Delete" moves to the trash instead of hiding.
- **No trash:** machine leftovers nobody curates (raw tool results, fetched
  pages, screenshots, shell output). They expire; see below.
- "Empty trash now" removes the trash at once, when space is needed sooner.
- A running task cannot be deleted; cancel it first. A crashed run must be
  able to resume (invariant 3), and only ended runs lose content.
- Who may delete: a person deletes their own items. Knowledge entries are
  company content: their author or the owner deletes them.
- The log records each step (trashed, restored, removed) with who and when.

### Expiry of tool results

Once a task has ended, its raw tool results are removed after a number of
days the owner sets in Settings. The answer, delivered files, each action's
record and the content's hash stay until deleted by hand. The proposed
default is 30 days, the same as the trash and the backup retention
(`[backup] keep_days`), so one date answers "when is it gone?".

### Moving existing content out

`magnus migrate`, run with the admin database login, moves the content in
existing events into the store and replaces it with references and hashes,
once. This is the only time event rows change, and it is done by the admin,
not the kernel.

Postgres does not shrink its files when rows are deleted; it reuses the
space. The migration ends with `VACUUM FULL` on both tables, which locks
them while it runs, so it runs while the appliance is stopped. Routine
removals need only the regular vacuum.

### Backups

Backups are encrypted copies, kept for `keep_days` (30). Deleted content stays
in older backups until they expire. Restoring a backup older than a deletion
would bring that content back. So restore re-applies a small list of removed
item ids, kept beside the backups, never in them.

## Invariants

- **3**, now: "the append-only event log is the source of truth. A crashed
  run resumes without repeating side effects." Its test
  `all_state_is_rebuilt_from_the_event_log_alone` rebuilds from the log.

  Proposed: "the append-only event log is the source of truth for what
  happened; the content it refers to is kept beside it and can be deleted,
  and once deleted it is gone. A crashed run resumes without repeating side
  effects."

  Tests to add:
  - the kernel still cannot change or delete an event;
  - a deleted item's content is gone from the store, and its hash and record
    remain;
  - a running task cannot be deleted;
  - state rebuilds from the log plus the store.

  Owner's approval needed.
- **2 (credentials)**: unchanged. Redaction runs before content is stored, as
  now.
- **4 (taint)**: labels stay on the record, not the content, so a deleted
  item's taint still counts for what it already influenced.
- **7 (verify)**: a deleted task's evidence goes with it. Its verdict and the
  record of each check stay.

## What stays risky

- Model providers keep their own copies of what was sent to them, under their
  own terms. Deleting in Magnus does not reach them.
- What a deleted item already influenced stays: a memory it wrote, an answer
  that quoted it (by the owner's decision, only the item is deleted).
- Deleted content survives in backups for up to `keep_days`.

## Build order

Each step is its own PR with tests and a DECISIONS.md line.

1. **The content store.** New events write content to the store and refer to
   it. Reading, the run inspector and the app work unchanged. Invariant 3
   tests pass; the new ones are added once the wording is approved.
2. **Trash and delete.** API and app for each item type; restore; removal
   after 30 days; "Empty trash now". Chat "Delete" moves to the trash.
   Installed-app test.
3. **Expiry of tool results.** The owner's setting, the expiry job, and
   "expired" in the inspector.
4. **Moving existing content out.** The migration and `VACUUM FULL`. Measure
   the log on strix-alpha before and after.
5. **Backups.** The list of removed ids beside the backups, re-applied on
   restore.

## Open decisions for the owner

1. Invariant 3's new wording, above.
2. The trash list, above.
3. The default for tool-result expiry (proposed 30 days).
