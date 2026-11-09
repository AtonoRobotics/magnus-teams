# The send reviewer (#179)

A removable module that judges every send before it leaves, and the
completion criteria no code check covers after the verifier. Full module
integration: `[reviewer] enabled` default off (the verifier precedent:
expensive per-use judgment is opt-in), per-model switches, dev registry
entry with ablation suite, acted counting via `Run.reviewed`.

Authority (the issue's bounds): block with a reason, or flag for the
person; nothing it returns lets through a send the kernel would ask
about. Structurally: the reviewer runs only when policy asks nothing. A
send the kernel asks about is reviewed by the person instead, who
outranks it. When the reviewer is off, unavailable, or its budget is
spent, sends proceed and criteria stay, as before.

- Block: denied at the boundary with the reason, plus a notice item on
  the task so the person sees why. The run continues: a corrected send
  is judged fresh.
- Flag: an approval card of the same shape as any ask, with rule
  `reviewer:flag` and the reviewer's reason, so the existing approve /
  reject path, role gates and UI all apply unchanged. The reviewer skips
  a send the person already cleared (extracted `covering_approval`
  matcher, shared with the approval stage), so accept-then-retry does
  not ask twice.
- Completion: holds are recorded as Verification by the reviewer;
  flags and unknowns stay for the person carrying the reviewer's reason.
  Could-not-run checks still go to the person unchanged (K37): the
  reviewer judges only check-less criteria. It never fails the work:
  doubt goes to the person.

No model override (unlike the verifier): a send under review is already
fully in context, so the run's own model judges. Schedules keep the
verifier only; enabling the reviewer there is a separate decision.
