# Aurora's glass blurs again

## Choice

Aurora and Obsidian 2 Aurora draw their islands as frosted glass, as the
mockup does: the navigation, the panel and its head, the composer, the
conversation's cards and bubbles, the hello and explore cards, dialogs, and
the stage's cards and canvas blur what is behind them (`backdrop-filter`,
18–24px, the values the theme shipped with). Forced colours turn the blur off.

This supersedes `2026-11-20-aurora-solid-surfaces.md`. That rule, and the
check in `desktop/tests/companion-panel-check.mjs` that failed any stylesheet
using `backdrop-filter`, were never approved by the owner; the owner asked for
both to be removed.

## What stays

- The default looks keep their solid, unblurred chrome
  (`2026-11-16-one-stylesheet-no-glass-left.md`, checked by
  `desktop/tests/app-looks.mjs`); this blur is scoped to the Aurora styles.
- No CSS `filter: blur()` on the ambient canvas or in entrance keyframes
  (`2026-11-20-aurora-entrances-keyed.md`). That filter was recomputed on every
  frame of the canvas and is part of the flashing fix, not a surface rule.
