# 2026-11-24 — Side + Bottom bento stage and Antigravity-style thread change history

## Context
Two bugs kept cards inline inside the chat window instead of composing around it:
1. `aurora-stage.css` queried `@container aurora-conv (min-width: 820px)` on `section.conv` itself, which declared `container-name: aurora-conv`. In CSS Containment, an element cannot match a `@container` query targeting itself, so the 2-column side layout never activated.
2. Process entries inside `#thread` (`.reqgroup.chatcard`, `.sub-agents` with `<details open>`, `.result` with `<details open>`) still rendered as full glass cards inline in the chat stream, and individual sub-agents had no dedicated bento tiles.

## Decision
- Declare `container-type: inline-size; container-name: aurora-main` on `.shell > main:has(> section.conv)` and switch the composed stage grid to `@container aurora-main (min-width: 820px)`.
- Split composed bento tiles cleanly across **Side Bento** (`.stage-bento`, right column beside `#thread` for primary focus cards: pending approvals + Live Workspace) and **Bottom Bento** (`.stage-bento-bottom`, full-width horizontal strip below `#thread` + Side Bento and above `#composer` for parallel sub-agents, parallel tasks, files, messages, and recent work). When `#main` narrows below `820px` (e.g. right sidebar open), both fold smoothly above the composer.
- Give each active or recent sub-agent its own live bento tile (`sub:<run>`) so parallel research tracks arrange side-by-side across the Bottom Bento.
- Compact inline task/sub-agent/result entries inside `#thread` (`compactThreadHistory()`) into borderless, Antigravity-style vertical change-history rows (`data-history="1"`) with auto-collapsed `<details>` on first render, keeping the chat stream clean and scannable while interactive work cards live in the Side and Bottom bento.
