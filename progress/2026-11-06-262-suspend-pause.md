# #262: admin suspensions need an admin; held schedules can be paused

`SchedulePaused` carries `admin`, the schedule keeps `paused_by_admin`; resume refuses non-admins on suspended schedules, and pausing a held schedule converts it to a kept personal pause (`controls.rs`, `state.rs`, `events.rs`, `person_admin.rs`, three `admin: false` sites in `kernel.rs`).

RED: `a_person_can_pause_a_schedule_held_on_them_and_it_stays_paused` fails unfixed (pause 409). GREEN locally: that test passes; `state.rs` replay unit test passes; `magnus` lib 87/87. The suspension assertions extend `admin_people.rs` `root_suspends_...`; they compile but cannot run on this box (its browser/OAuth setup times out in `loopback_within`, the #313 environment), so CI is their gate.
