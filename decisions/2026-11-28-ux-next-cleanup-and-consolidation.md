# 2026-11-28 — UX Next Cleanup & Consolidation Refactor

## Choice
1. **Phase 0 Dead-Code Removal First**:
   - Deleted the 5 unreachable Aurora stylesheets (`aurora.css`, `aurora-stage.css`, `aurora-widgets.css`, `aurora-rail.css`, `aurora-compact.css`) whose `[data-style="aurora"]` / `[data-style="obsidian-aurora"]` selectors could never match after `theme.js` consolidated `aurora` → `nova`.
   - Deleted dead `aurora-pages.js` (and its unused `.aurora-page-close` rules in `nova-shell.css`), which was unreachable in both `nova` (`pageCloseButton: false`) and `classic` (`stage: false`), plus the dead `obsidian-aurora` branch in `aurora-orb.js`.
2. **Replace & Unify Instead of Layering**:
   - Unified `/` slash commands + `@` typed context mentions in `chat-composer.js` + `composer-commands.js`.
   - Wired inline plan/draft commenting (`inline-comments.js`) and assistant refinement micro-actions directly into `plan-surface.js` and `message-controls.js`.
   - Extracted and deleted the duplicated inline `⌘K` palette from `app-shell.js`, unifying personal + workspace commands into `command-bar.js`, and wired `j`/`k`/`a`/`d` keyboard Inbox triage + 5-second `UndoToast` directly into `app-inbox.js`.
   - Added Split-Screen Co-Edit Canvas (`split-coedit.js`) directly on top of `coedit.js` primitives.
   - Refactored `banners()` in `app.js` to feed a single compact `WorkQueue` strip (`work-queue.js`), keeping full-width banners strictly for errors/offline states.
3. **Real Browser E2E Verification**:
   - Added headless-Chrome Playwright E2E verification (`desktop/tests/ux-next-e2e.mjs`) booting the real `desktop/ui/index.html` bundle to verify zero runtime/asset errors and all interactive flows end-to-end.

## Reason
- Layering new UI patches on top of retired stylesheets and duplicated palette/banner/mention code caused recurring regressions and stylesheet bloat.
- Removing ~2,900 lines of dead code first and replacing duplicated paths directly keeps every module well under 800 lines, shrinks the shipped asset bundle, and ensures every planned feature is verified in a real browser DOM.
