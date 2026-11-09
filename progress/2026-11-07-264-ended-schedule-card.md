# #264: an ended schedule's card says it ended, with its latest run

## What changed

- `crates/magnus/src/state.rs`: `Schedule.ended`; `ScheduleEnded` keeps
  and marks (clearing pause), `ScheduleChanged` revives,
  `ScheduleRemoved` still removes; `finished()` none once marked.
- `crates/magnus/src/api.rs`, `person_admin.rs`: `ended` in the schedule
  JSON.
- `crates/magnus/src/controls.rs`, `person_admin.rs`: pause, resume and
  admin suspend refuse on an ended schedule.
- `crates/magnus/src/kernel.rs`: the sweep skips ended schedules when
  holding runs for their person; comment updated.
- `desktop/ui/app.js`: Ended chip + reason + latest run on the card
  (no acts), ended mark in facts, no Next run once over, ended sort
  last in the list.
- Tests: state projection unit test; new `ended_schedules.rs` (until
  end + reason, pause/resume refused + edit revives, out-of-runs with
  latest run linked); `live_cards` fixture + `live-cards.mjs` step for
  the card; the two `schedules.rs` assertions that pinned ended=gone
  now pin the mark.

## Verification

- Unit test RED (no `ended` field) then GREEN; until tests RED (90s
  timeout, never listed) then GREEN in 4s.
- `live_cards` GREEN locally in a real browser (all steps incl. the new
  Ended step); the firing test GREEN in 61s.
- `magnus` lib 93/93; node smoke of the actual `scheduleLine` /
  `scheduleText` asserts Ended + reason + Last result + Its runs with
  no Cancelled and no acts; `node --check` on both JS files; no new
  clippy lints.
- `schedules.rs` firing flows flake on this box (run completion, same
  disease as #313; the edited ended-marker assertion itself passed);
  CI gates those files.
