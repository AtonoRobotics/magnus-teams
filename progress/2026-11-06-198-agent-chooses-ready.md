# Agents choose the next ready issue (2026-11-06)

CLAUDE.md's "Work starts from an issue" no longer waits on the owner to add `ready`: the agent promotes the next issue from `queue` itself (or claims it straight to `in-progress`), by impact with a red master or failed release first, saying why in the claim comment. The pull request template's checklist matches. One claim per issue, one shared-file change in flight, and the 6-PR cap are unchanged.
