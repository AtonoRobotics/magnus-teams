# #512 activity-updates budget (2026-11-08)

- Master red since 12c13e3 fixed at the harness: observation wait 60s → 180s in `workspace_layout.rs` with the evidence in a comment.
- Diagnosis path: reproduced green on clean master locally (~6s); CPU saturation alone did not break it; #483's green full-suite run passed it in 2.7s; all three red runs sit in a 3-4 concurrent-suite pile-up (CI timeline). Exonerated #504 (passes with it idle), #445 (same hours green/red), and a needsMe/Activity scare (stale binary after branch switch — rebuilt, green).
- Verified: `workspace_layout` 2/2 on the fix branch with fresh bins.
- Unblocks: #507 merge, then #508 slice B.
