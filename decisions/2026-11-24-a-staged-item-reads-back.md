# A staged item's event reads back, and its words are content

**Choice.** A staged item's `kind` (its `StageKind`) is written as
`stage_kind`. A created item written before, whose `kind` stands where the
event's own tag should be, is read as the current form
(`staging::current_staged`, called from `log::parse_event`). A staged
event's `sentence`, `input`, `shown` and `provenance` are content: kept
beside the log and deleted with it.

**Reason.** `Ev::Staged(Change::Created(item))` writes the item's fields
into the event's own object, and `Ev` is tagged by `kind`. The item's
`kind` replaced the tag. The event then did not parse, and `Log::load`
stops at an event that does not parse, so a server that had staged one
message would not start again. With no tag, the row's `kind` column was
empty too, so the message's text and recipients were never split off as
content and could not be deleted. A row written so still holds its words
inline; the reader makes it load, but it cannot move those words out of
an append-only row. That needs the owner (see the report of 2026-11-24).
The same clash happened before with a lane's `kind`
(`current_lane_dispatched`).
