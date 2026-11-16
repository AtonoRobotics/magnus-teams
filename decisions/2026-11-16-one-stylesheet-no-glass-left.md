# One app stylesheet, each selector once, and nothing of glass left

## Choice
`2026-11-15-remove-glass-material-and-merge-app-css.md` appended
`modern-ux.css` to `app.css` and left 59 selectors defined twice: the old
rule early in the file and the Antigravity override at its end. That is the
same layering it set out to remove, in one file instead of two. Finishing it:

- Every duplicated selector in `app.css` is one rule. The declarations that won
  before still win: the fold was checked by rendering the signed-in app with
  the old and the new stylesheet and comparing the computed style of every
  element in the conversation, Needs you, Activity, Settings and Appearance, in
  light and dark, at 1440px and 390px, until nothing but the run's own noise
  differed (a notice and the device list, which change between any two runs).
- What was left of glass goes: the pre-script `html` fallback reads `--bg` in
  the colors the window starts with (`look.rs` `SOLID_LIGHT`/`SOLID_DARK`,
  checked by `solid-start-check.mjs`) instead of `--backdrop` in the retired
  Workspace colors; the `html[data-backdrop="native"]` rule, the
  `data-backdrop` attribute, the `--backdrop`, `--main-edge` and `--panel-edge`
  tokens, the never-set `--main-gap`/`--main-radius`/`--main-shadow` uses and
  the `backdrop-filter: none` resets that cancelled glass are removed, with the
  comments that described glass.
- `window-vibrancy` is no longer a dependency of the desktop app (Tauri keeps
  its own).
- The tests that still chose `material` now cover the two looks:
  `app-looks.mjs` (every surface solid, nothing blurred, no material choice,
  the Emerald accent since Teal no longer exists), `looks.mjs` (Alabaster and
  Obsidian reference values instead of the retired Workspace ones),
  `workspace-layout.mjs`, `chat-app-card.mjs`, `approval-app-card.mjs`,
  `connectors.mjs`. `native-material-check.mjs` is `native-background-check.mjs`
  and checks that a failed host call does not stop the next.

`Theme.scope()` still reads a saved record with `material` and writes it back
without it: a retired field keeps a reader.

## Reason
The owner asked for the old theme system to be removed and replaced, not
overridden. A selector defined twice is an override; a fold that changes what
is drawn is a redesign nobody asked for, so the fold is proven by computed
style rather than by reading.
