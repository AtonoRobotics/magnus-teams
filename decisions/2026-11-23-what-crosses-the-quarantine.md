# What crosses the quarantine, and who is quarantined

Follows 2026-11-23-quarantine-recorded-with-the-run.md.

**Choice.**
1. The quarantine module alone decides what of an untrusted answer reaches a
   quarantined planner (`quarantine_tool_output`, `crossing`). Its text goes
   behind a handle as before. Of its data, nothing the planner is shown
   crosses: a picture (`image_b64`, which the run's context sends as an image)
   is removed, and the planner is told `picture_withheld`. The rest of the data
   (attachment records, handles) stays for the kernel's own use.
2. One function decides whether a run nobody watches is quarantined, and under
   which schema: `Kernel::unattended_quarantine`. A personal schedule's firing
   and a team routine's both ask it. A run is quarantined when its answer goes
   to others unasked (a routine's, posted to its room), or when what it asks
   names a writable destination someone other than its person (the rule
   schedules had). The routine's run carries the schema in the `TaskCreated`
   that creates it, through its room mission's lane (`LaneSpec::quarantine`,
   set only by the server, never from a lane's body). A follow-up on a quiet
   thread is held for its person to release, so it is not quarantined.
3. Handle numbers are reserved, not counted. A quarantined result reserves a
   block (one number for each field its schema declares, and one for the
   body) for its tree in one locked step (`reserve_handles`), seeded from the
   highest handle the tree has recorded, so a restart continues after them.
   Numbers left unused leave gaps, which name nothing. Trees that have ended
   are let go from the reservation map.

**Reason.** A quarantined run's untrusted text was held back but its pictures
were not: an opened attachment or a screenshot of a hostile page reached the
planner, words in it included. Team routines run unattended and post to a room
others read, yet were never quarantined. Since sibling sub-agents run at once,
two results counted the tree's recorded handles before either was written and
both made `$q1`, so a send could carry the other sibling's text.
