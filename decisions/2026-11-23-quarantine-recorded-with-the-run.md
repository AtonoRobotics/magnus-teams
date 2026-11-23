# Quarantine is recorded with the run, and handles expand at the boundary

Follows 2026-11-21-quarantine-runtime-and-persisted-budgets.md, which kept
quarantine in a process-wide set.

**Choice.**
1. Whether a run reads untrusted output under quarantine is part of its pinned
   configuration: `RunConfig::quarantine`, the extraction schema, `None` when
   not quarantined. `fire_schedule` decides it before it creates the run
   (`Kernel::schedule_quarantine`, the same rule as before) and passes it in
   `NewRun`, so it is in the `TaskCreated` that creates the run, before the run
   is woken. A restart, an update's resume and a replay rebuild it from the
   log. Sub-agents inherit it through the top run (`quarantine_of`). The schema
   is pinned as resolved (the schedule's own, or the typed-only default).
2. The process-wide `QUARANTINE` static and its 2048-entry cap are gone.
3. An opaque handle's text is recorded in the data of the tool result that
   made it, under `quarantine_handles` (`{"$q1": "<raw text>"}`). The model is
   never shown a result's data, the data is a content field (erased with its
   person), and the handles of a run tree are read back from its results.
   Handles are numbered after the ones the tree already has: a run's calls go
   one at a time, and the number is taken just before the result is written.
4. At the boundary, right after the tool is identified
   (`Kernel::expand_handles`): in a quarantined run, a handle in a destination
   field (to, cc, url, host, channel, and the like, or inside any address) is
   denied (N12). In an action that changes something and reaches others (not
   `Audience::Private`), each whole `$qN` becomes its text. Everything after
   that point sees the expanded body: destinations, policy, Auto Review and
   `outgoing_finding`, the approval card, and the recorded intent. A read, a
   sub-agent's goal or a write to the person's own space keeps the handle as
   written, so raw text never reaches a planner through them.

**Reason.** The static was empty after a restart or an update's resume, so a
resumed quarantined run handed raw untrusted output to its planner. It was
set only after `create_run` had woken the run, so a first step could run
unquarantined. Its cap cleared every root once it filled. Nothing expanded the
handles the model was told to write, so recipients got a literal "$q1".
