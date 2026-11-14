# A daily schedule's time is its person's, in their own time zone

Asked of the owner: "Should daily schedule times follow each person's own
timezone instead of the server's? That would change when existing schedules
fire." The owner answered (2026-11-14): "1 yes".

Decided: a schedule's `daily_at` is read in the zone its person's
preferences name (`timezone`, the same preference `proactive` reads for
working hours), and in the server's own zone (`chrono::Local`, as before)
when they name none or one that is not a zone. Unlike `proactive::tz_of`,
the fallback is the server's zone, not UTC, so a person with no zone set
fires exactly as before. The zone is held on each `Schedule` and kept
current by `State` as the person changes it, so every reader of `next_due`
(the Schedules view, the admin views, the sweep) sees the same time;
`next_due_in(zone)` takes the zone explicitly. Interval schedules and team
routines do not depend on a zone.

What changes for people: someone whose preferences already name a zone
different from the server's sees their daily schedules move to that zone's
wall-clock time. The schedule card's "Every day at 08:00" now means 08:00
where its person is. No stored event changes shape.

Tests: `schedule_zone_tests` (a person in Tokyo fires at 08:00 Tokyo time, a
schedule made before the zone was set follows it, Los Angeles clock changes,
no zone and a bad zone keep the server's clock).
