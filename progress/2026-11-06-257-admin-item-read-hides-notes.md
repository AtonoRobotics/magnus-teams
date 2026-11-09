# An admin's task-item read never returns relationship notes (2026-11-06)

The admin console's read of one task no longer takes a chat turn's id (404, as the tasks list does), and `relationship_note`/`disagreement` calls are redacted — name kept, words replaced — in the admin's task timeline, the developer run inspector, and the raw log whenever the reader is not the run's person. The person's own timeline still shows their words.
