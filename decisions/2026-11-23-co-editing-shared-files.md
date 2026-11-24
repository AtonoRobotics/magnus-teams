# Co-editing shared files

**Choice.** Sharing a file to a shared project, a member picks "Share a
copy" or "Co-edit". A copy works as before: it stays theirs and nobody else
writes it. A co-edited file belongs to the project (`File.coedit`, `by`
empty) and changes only by versions; the sharer keeps their own copy in
their library as it was when shared, as `share_held` already did. Code
stays in GitHub. The kernel is `crates/magnus/src/team/coedit/`.

- **Versions.** Every change is a `Change::Coedit` team event whose
  content is one `Step`: a `Version` with its number, its base, its author
  (a person, or a person's agent and its run), its diff against its base,
  and its bytes. Everything but the workspace is content, so it is kept
  beside the log (invariant 3) and the history is whole. A revert is one
  more version, made on the head. A new kind, so no event kind changed.
- **The version check.** A person's write (`PUT /files/{name}?base=N`)
  must say its base. On the head it lands. On an older base it merges by
  format against the head: text by paragraph (three-way, split on blank
  lines), a sheet (`.csv`, `.tsv`) by cell then its formulas are worked
  out again, records (`.json`) by record (`id`) and field, and a binary
  file not at all. A change that conflicts is a 409 naming each part with
  both versions; it is kept as the member's own `Proposed` step, so nothing
  is lost, and they keep a version of each part
  (`POST coedit-changes/{id}/land`) or throw theirs away. Writes are
  serialized (one lock), so the check and the commit are one step.
- **Formats.** By extension: md, markdown, txt, text, rst, adoc, org, tex
  are text. Anything else, `.docx` and `.xlsx` included, is binary for now:
  their parts are zipped XML, and a merge that is wrong there loses work
  silently. A sheet's rows are positional, so two people inserting rows
  above the same row conflict instead of landing in a guessed order.
- **Binary check-out.** A binary file is checked out to one editor for
  `CHECKOUT_LEASE_SECS` (eight hours, a working day; checking out again
  renews it). While it holds, nobody else's write lands and others read the
  last version. A write needs no check-out of one's own when nobody holds
  one: the version check alone refuses a stale base.
- **An agent's edit.** An agent edits only inside a member lane, a request
  in the project's room (`team_propose_edit`, after `team_read_file`). Its
  edit is a proposal attached to the lane: it lands only when the lane's
  result is released. `Kernel::release_automatically` judges it with the
  result (the scrub, the run's taint, the outgoing classifier on the
  edit's new parts, and Auto Review, told the edits go with the result);
  when it posts, the edits commit under the version check, and a conflict
  then holds the lane in its owner's Airlock with `held_because` instead.
  A lane that waits (the requester's own ask or a standing release did not
  cover it, or a check held it) keeps its edits with it, shown on its
  owner's card only, and the owner's release lands them. A conflict found
  when the agent proposes goes back to its run, which reads the file again
  and proposes on the new version. No second gate.
- **Reading.** `team_read_file` returns a shared file and, for a co-edited
  one, its version. Its result is untrusted when anyone other than the
  run's person wrote any version (`owner::from_others`).
- **Sending.** A send stays a send, and payments still ask every time. Its
  receipt names each co-edited file the run read before the send and whose
  name the send carries, at the version read last (`versions_sent`).
- **Erasure (owner, 2026-11-23).** When an editor is erased the project
  keeps the content and removes their name: `unsend_all_of` writes each
  version they wrote again as `Reattributed`, "a former member" its author
  and no run, and removes the originals' content. Their open changes and
  check-outs are theirs (`content_owner`), so they go with them. A version
  is no one's content, so nobody else's erasure takes it.

- **Limits, and what a merge keeps (review, 2026-11-23).** A co-edited
  sheet has at most `MAX_SHEET_ROWS` rows (20,000), `MAX_SHEET_COLUMNS`
  columns (1,000) and `MAX_SHEET_CELLS` cells (200,000); a bigger one is
  refused when written, since its merge and diff run while every other
  co-edit write waits. A merge or diff walks only the cells some version
  has. A formula goes at most `MAX_FORMULA_DEPTH` (256) steps deep, its
  parentheses, signs, calls and the references it follows counted
  together, and shows `#DEPTH!` past it: before, a cell of 50,000
  parentheses overflowed the stack and took the server down on every read.
  Working out one sheet's values reads at most `MAX_RANGE_READS` range
  cells in all; formulas past it show `#LIMIT!`. Records are found by id in
  one step, so a merge of many is linear. A text merge keeps every byte it
  did not change: each paragraph carries its own line endings and the blank
  lines after it, an untouched one is written as the head has it, and a
  blank line is added only where two paragraphs would otherwise run
  together.
- **Who may land a change.** A change lands only while its author still
  sees the file (`sees_file`): a member who left the project, or the
  workspace, lands neither a change they kept nor their agent's edit with
  its lane, and such a change is dropped instead, so no answer shows them
  the file as it is now. A new name is found free and the file shared under
  it in one step, under the same lock as every co-edit write. A file's size
  is its head version's; an erasure's rewrite of an old version leaves it.
- **Records' numbers.** A records file is read and written as JSON values
  (`serde_json` without arbitrary precision), so an integer beyond 64 bits,
  or a float written with more digits than a double holds, comes back
  rounded after a merge, and the merged file is pretty-printed. Only a stale
  merge rewrites the file; a change made on the head lands byte for byte.
  Keeping such numbers exactly would mean merging the JSON as text, which
  is left for when a project needs it.

**Reason.** The owner's design, Co-editing shared files. Versions in the
log make a co-edited file's history whole and every change attributable,
and a stale change that is rejected rather than overwritten is the only
way nothing is lost. Taking the lane's path for an agent's edit keeps one
release gate: an edit reaches the project exactly when the result it came
with may, under the same checks. The receipt matches a file by name in the
send because a send's input is the only record of what went; a send that
carried a file the run never read names no version, which is the honest
answer.
