# 2026-11-23: A team knowledge entry knows who last wrote it, and who read it

**Choice.**
- An entry records who last wrote its words: `Entry::edited_by`, set by
  a curator's correction that changes its statement or its type (a
  correction that only looks at it, or changes its relations, writes no
  words). It is an optional field of the entry the `team` change already
  carries, absent until set, so every entry written before reads as its
  author's and no event kind changes meaning.
- `team_read_knowledge` marks a run untrusted when anyone but its person
  wrote, owns or last wrote an entry it was given (`Entry::writers`), and
  the export names the last writer (`last written by`).
- A read of a team base says which entries it returned, as the personal
  reads do (`read_copies::read_data`), and removing an entry names those
  results in its `Removed` (`State::reads_of`), so the entry's text leaves
  the runs that read it, the log and the backups.

**Reason.** A correction replaced the words and kept the author, so a
curator's rewrite reached the author's runs as the author's own words
and the export credited it to them. Removing an entry promised it was
gone from the log and the backups, but every run that had read the base
kept the statement in its tool result.

**Not changed.** Entries belong to the workspace on a person's erasure
(2026-11-13), the corrector's name included. A correction clears the
entry's flags, as before. Corrections made before this release do not
say who made them and still read as the author's.
