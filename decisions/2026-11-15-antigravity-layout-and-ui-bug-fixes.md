# Obsidian Layout Geometry & UI Bug Fixes

## Context
Previous UI updates changed `desktop/ui/theme.js` to Obsidian Obsidian/Alabaster tokens, but left the legacy Workspace/Material layout geometry intact (double dark top bars, a 16px dark splitter trench, right-aligned user chat bubbles, 44px desktop composer buttons, and 60px task cards), and left several regressions (`showNewChat` overwriting the hero greeting, `admin.css` clobbering pill tabs, undefined `--elevated`/`--surface-2` tokens, and `html-frame.js` missing from `crates/magnus/src/ui/assets.rs`).

## Decision
- Fixed `showNewChat` in `desktop/ui/app.js` to render `ChatView.listHtml([], view)` (restoring the personalized hero greeting and starter chips on `#/chat/new`) and deferred `Cmd/Ctrl+K` to `TeamCommand` on `#/team/` routes.
- Updated `desktop/ui/admin.css` to use the Obsidian pill `.admin-sub` styling, replaced undefined `--elevated`/`--surface-2` tokens with `--raised`/`--seg`, removed the inverted split-grid duplicate in `desktop/ui/app.css`, and enforced 44px coarse-pointer targets on accent swatches, `.inspector-pill`, and `.kill-menu > summary`.
- Applied Obsidian layout geometry in `desktop/ui/app.css`: unified `.workspace-bar` and `.conv-head` onto `--main`, painted `#workspace-splitter` as a seamless 1px hairline while preserving the 16px track and 44px coarse-pointer hit area, converted `.msg.me` to full-width tonal prompt cards, compacted desktop composer controls (32px) and step timeline rows (36px), and added disclosure chevrons to `.nav-project`.
- Added `html-frame.js` to `crates/magnus/src/ui/assets.rs`.

## Reason
Delivers the intended Obsidian layout and component proportions rather than recoloring the legacy Workspace chrome, while preserving all layout, contrast, and coarse-pointer accessibility invariants.
