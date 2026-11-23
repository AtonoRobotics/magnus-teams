# UI review improvements: UX polish, performance, accessibility, and app.js modularization

**Date:** 2026-11-23

## Decision
Implement the three-batch UI polish pass across `desktop/ui/` and `crates/magnus/src/ui/assets.rs`:

1. **UX & interaction fixes**
   - Add keyboard navigation (`ArrowDown`/`ArrowUp`, active highlight, combobox ARIA) to `@` mention autocomplete, a drag-depth counter to stop drop-zone flicker, clipboard paste-to-attach, and formatted file sizes on attachment chips (`desktop/ui/chat-composer.js`, `desktop/ui/chat.css`).
   - Unify `⌘K`/`Ctrl+K` to always open `#workspace-search` (`openPalette()`), merge in shared-workspace actions via `commandsFor`, add combobox ARIA, `scrollIntoView`, and backdrop click-to-close, and remove the unreachable `TeamCommand.open()` dialog and its unused `.team-command*` styles (`desktop/ui/app.js`, `desktop/ui/team-command.js`, `desktop/ui/teams.css`).
   - Preserve the Settings filter input and focus across tab switches, add keyword matching and an empty state, and restore the route hash when dismissing Settings opened from `#/models` (`desktop/ui/app.js`, `desktop/ui/pages.css`).
   - Allow backdrop click-to-dismiss on `ask()`/`askText()`, pause actionable toast auto-dismiss timers on hover/focus, enable `Ctrl+B` sidebar toggle outside Aurora stage mode, add a keyboard shortcuts modal (`⌘/` or `?`), and replace remaining retired `"team agent"` / `"Shared Computer"` labels (`desktop/ui/team-panel.js`, `desktop/ui/aurora-stage.js`, `desktop/ui/screen.js`).

2. **Performance, polling & asset size**
   - Add in-flight guards to `refresh()` and `Notices.poll()`, slow polling when `document.hidden` with immediate foreground catch-up on `visibilitychange`, and move `/v1/version` checks to a 30 s cadence (`desktop/ui/app.js`, `desktop/ui/notices.js`).
   - Throttle `aurora-avatar.js` `pointermove` reads with `requestAnimationFrame`, cache parsed approval answer HTML in `aurora-stage.js`, route `aurora-explore.js` through `AuroraCore.onDom`, and merge three separate `document.body` `MutationObserver`s in `app.js` into one.
   - Convert the six Explore JPEGs (2.22 MB) to resized WebP assets (84.9 KB total, 96.3% smaller) and serve them as `image/webp` (`desktop/ui/discover.js`, `crates/magnus/src/ui/assets.rs`).

3. **Accessibility, stacking tokens & `app.js` split**
   - Add a visually-hidden polite live region (`[data-chat-announce]`) in `desktop/ui/chat-view.js` to announce new assistant replies.
   - Define a centralized `--z-*` stacking scale in `desktop/ui/base.css`, resolve stacking collisions (`#toasts` on `--z-toast`, `#stop-banner` on `--z-banner`), and consolidate duplicate `:focus-visible` and `@media (any-pointer: coarse)` blocks.
   - Expose `Workspace.openDrawer()` so `desktop/ui/aurora-rail.js` calls the shell API directly instead of clicking a hidden button.
   - Extract four cohesive domain modules out of `desktop/ui/app.js` (`composed-frames.js`, `signin.js`, `tasks-schedules.js`, `settings-sections.js`), shrinking `app.js` by ~880 lines while preserving plain-JS loading with no build step.

## Reason
All changes follow the Magnus Agent Computer spec and Code Standard (no build step, single-place CSS rules, named duration constants, WCAG AA contrast, pure peer collaboration with personal agents), eliminate concrete UX bugs and unnecessary network/DOM work, cut over 2.1 MB from the embedded binary assets, and reduce `app.js` monolith size. Covered by `desktop/tests/ui-polish-check.mjs`.
