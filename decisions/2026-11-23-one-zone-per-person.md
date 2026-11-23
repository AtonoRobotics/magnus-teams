# One zone per person, read one way

**Choice.** `cadence::zone_of` is the only reading of a person's time zone: the IANA zone their preferences name, else the server's. Their daily schedules, working and quiet hours, the day their interruptions and reviews count in (`proactive`, `state::inbox`), the zone Settings shows, and what the model is told all go through it.

- The one fallback is the server's zone, as the owner chose for schedules on 2026-11-14 ("a person with no zone set fires exactly as before"). Quiet hours, working hours and the daily counts, which fell back to UTC, now fall back to it too.
- The server's zone is named once, when the config is read (`Config::server_zone`): `TZ` when it names a zone, else the zone `/etc/localtime` links to, else `/etc/timezone`, else UTC, the order `chrono::Local` reads the clock in. Settings shows that name to a person who set none, where it showed "UTC".
- The tool descriptions say `daily_at` is in the person's own time zone, not "server local time". `create_schedule`'s result names the zone ("It runs every day at 08:00, America/Los_Angeles time"), and `edit_schedule`'s answer carries it as `time_zone`.

**Reason.** Schedules fell back to the server's zone while quiet hours, working hours and commitments fell back to UTC, Settings showed UTC, and the model was told daily times were the server's. A person with no zone set on a server in Los Angeles had quiet hours eight hours off from their schedules, and the model had no way to know which clock "08:00" meant.
