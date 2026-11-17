# 2026-11-15 — Obsidian Shell, Navigation, and Timeline Redesign

## Decision
Replaced the legacy Magnus conversation, navigation, and card presentation templates and CSS in `desktop/ui/app.js`, `desktop/ui/personal-chat.js`, and `desktop/ui/app.css`:
- **Navigation (`nav.side`)**: Sentence-case section labels (`Projects`, `Conversations`, `Teams`), compact `30px` filled search input, `32px` `+ New conversation` button, `28px` conversation rows, omitted empty `"No projects yet."` placeholder, and `displayTitle()` mapping raw internal `"This turn is not a message from the person..."` titles to `"Automated check"`.
- **Header (`#conv-head`)**: Collapsed empty `#workspace-bar` in conversation mode, removed the stacked `No project` subtitle and empty project selector when no projects exist, and kept right-side actions to quiet `Activity` + trash controls.
- **Cards & Step Timeline (`tasks`, `result`)**: Replaced the stacked bordered `.reqgroup.chatcard` and `.result` boxes and redundant green `Done` pill badges with a borderless hairline vertical timeline, hiding `From this request` on single-task turns and placing `Outcome` inline.
- **Messages & Composer**: Made `.msg-actions` hover-only and omitted them from `tasks`/`result` entries, moved `Share to team` to a quiet ghost action, hid `.msg.me .byline` on user prompt cards, and styled `.say` as a rounded-16px elevated surface with a `210px` model selector.

## Reason
CSS-only token adjustments could not overcome the legacy multi-card DOM structure, noisy inline action bars, and double-stacked headers. Replacing the DOM templates alongside deduplicating conflicting CSS rules aligns the rendered UI with Obsidian while preserving all `data-*` contracts and accessibility/contrast invariants.
