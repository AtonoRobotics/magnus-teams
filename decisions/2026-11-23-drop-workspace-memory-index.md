# 2026-11-23: Drop the workspace memory index

**Choice.** The in-memory `workspace:<id>` index of released contributions
(`MissionLedger::workspace_memory`, `WorkspaceMemoryItem`) is deleted, with
`MemoryScope` and `WORKSPACE_MEMORY_MIGRATION_SQL`. Releasing a lane no
longer writes anything to it. This reverses part 2 of
`2026-11-22-unified-workspace-rls-memory.md`; the two-role mapping
(`TeamRole`) stays.

**Reason.** Nothing read it: its only reader, `GET /v1/workspaces/{ws}/memory`,
was deleted on 2026-11-22 (`2026-11-22-retire-team-agent-runs.md`), and
since then it was written on every release and read only by tests and by
erasure. It was one more copy of each contributor's words to keep track
of. The migration SQL never ran (nothing called it), so no table or column
is left behind; `public.memory_index` and its own migration are untouched.
