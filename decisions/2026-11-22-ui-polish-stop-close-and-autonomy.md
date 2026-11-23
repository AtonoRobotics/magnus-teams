# Magnus UI polish: instant Send-to-Stop square, close deduplication, harmonized sizing, and autonomy UX

## Choice

Implement all three slices from the desktop UI review (`magnus-ui-review.md`):

1. **Instant Send → Stop square & mid-run Steer** (`desktop/ui/chat-composer.js`, `desktop/ui/personal-chat.js`, `desktop/ui/team-conversation.js`, `desktop/ui/app.js`, `desktop/ui/chat.css`, `desktop/ui/aurora.css`):
   - Define `UI.stop` and `UI.mic` directly in `app.js`'s `UI` icon table (fixing the script-load-order bug where `chat-composer.js` ran before `UI` existed, causing `icon("stop")` to render empty).
   - Flip `button.send` immediately to `.is-stop` (high-contrast Stop square in all themes including Aurora) on submit via `record.sending || record.pendingRun || record.runningIds?.length`, carry optimistic `pendingRun`/`runningIds`/`cancelRequested` across `#/chat/new → #/chat/:id`, broaden active status detection across turns/tasks/sub-agents (`!ENDED.includes(status)`), support `Esc` to stop when empty, wire `stopRun` in team chat, and expose mid-run `steerRun` (`Server.tasks.steer`) alongside a companion `.say-stop` button when drafting while Magnus works.
2. **Deduplicate `×` / `Close` across pages and panels** (`desktop/ui/aurora-pages.js`, `desktop/ui/app.js`, `desktop/ui/layout.css`, `desktop/ui/aurora-compact.css`, `desktop/ui/pages.css`):
   - Hide `#workspace-bar [data-close-work]` whenever `#detail` is visible (`m.split || region === "work"`) across all themes so split and single-view panels never stack two close buttons.
   - Exempt Team rooms (`#/team/...` / `.team-main`) and Settings/Admin modal routes (`#/settings`, `#/admin`, `#/models`, `#settings-modal[open]`) from `aurora-pages.js`.
   - Replace `#settings-modal`'s primary `Done` button with a standard `32×32` `×` close button (`.settings-close`), harmonize `#detail .detail-head .close` from `24px` to `32px`, harmonize Apps' `.lclose` from `44px` to `36px` on fine pointers (`44px` preserved on coarse touch), and give `.explore-hero` `56px` right clearance so `.aurora-page-close` never overlaps hero buttons.
3. **Harmonized sizing, spacing & autonomy UX** (`desktop/ui/chat.css`, `desktop/ui/aurora.css`, `desktop/ui/aurora-stage.css`, `desktop/ui/pages.css`, `desktop/ui/admin.css`, `desktop/ui/layout.css`, `desktop/ui/aurora-rail.js`, `desktop/ui/aurora-rail.css`, `desktop/ui/asks.js`, `desktop/ui/aurora-stage.js`, `desktop/ui/app.js`):
   - Standardize desktop control scale to `32px` (header controls, panel close, secondary composer tools), `36px` (primary Send/Stop button, page close), and `40px` (left rail icons), with symmetric `10px 20px` `.conv-head` padding, `34px` Explore tabs matching `#explore-search`, `300px` featured Explore card max-height, `1.6` answer body line-height, balanced New-chat hero vertical rhythm, `1120px` ultrawide cap on `.admin-cards`, and a compact frosted `.jump-latest` pill.
   - Add left-rail active-work/approval badge (`.aurora-rail-badge` on `Conversations`), risk-tier impact pill (`.ask-tier`) on approval cards, inline `✓ Verified` receipt pill on completed answers, live tab filter in `#settings-modal`, instant `⌘K` jump & command palette in `#workspace-search` (with arrow-key navigation + full server search fallback), and one-click `Undo` toast on soft-delete Trash.

## Reason

Real-browser inspection caught an empty Stop SVG (load order), five duplicate/misaligned close controls across themes and viewports, six inconsistent desktop icon button sizes, and missing mid-run autonomy affordances (steering, background work indicator, risk-tier badges, quick jump, and trash undo).

## Verification

- All `desktop/tests/*-check.mjs` checks (including expanded `aurora-stability-check.mjs`, `stylesheet-check.mjs`, `globals-check.mjs`, `theme-check.mjs`, `asks-check.mjs`, `companion-panel-check.mjs`), `ci/ratchet.sh size`, and `cargo fmt --all --check` pass.
- Verified end-to-end in headless Chromium (`1280×800`, `960×800`, `390×800`, Obsidian 2 Aurora & Obsidian 2): instant Stop square with `<rect>`, mid-run Steer + companion Stop, exact single close control across every view and viewport, harmonized `32/36px` control sizes, rail badge, `Irreversible` tier badge, `✓ Verified` pill, Settings filter, and `⌘K` instant jump to Explore with zero console errors.
