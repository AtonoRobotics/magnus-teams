# #178: scoped drafting, content provenance at the send gate

## Changed

- `crates/magnus/src/boundary.rs`: draft writes
  (`create/reply/forward/update_draft`) run a scoped step after the gate
  allows them — a model call with no tools over the target thread (read
  by the kernel) plus the person's task text and chat messages — and the
  call carries the scoped body. Headers stay the run's. Any failure
  falls back to the run's text with no sources, as before.
- Same file: content provenance in the gate — a send of a scoped draft
  whose every recipient is a participant of its recorded threads passes
  the Rule-of-Two and audience taint asks (policy asks stand).
- `crates/magnus/src/events.rs`: `Ev::DraftScoped { run, person,
  draft, threads, participants, usage }`, in the run accessor and the
  content list (participants).
- `crates/magnus/src/state.rs`: `draft_sources` map, charged with the
  other aux calls, erased with the person.
- Tests: `scoped_drafts.rs` (a reply to A after reading A and B records
  only A and sends to new Bob with no card, carrying no B); unit pins
  for the entitlement rule and the state record.

## Verification

- New e2e RED on pristine code (the send asked), GREEN with the fix.
- Lib 110/110; `draft_lock` 3/3, `sent_provenance` 1/1,
  `directory_provenance` 1/1, `modules` 3/3, `aux_budget` 4/4 locally.
- `gmail` 0/8 locally, pristine-identical (the box runs no microVMs;
  verified by stash); green in CI, which gates. The three gmail tests
  that write drafts fall back deterministically (their scripts answer
  the scoped call with a tool block, which carries no text).
