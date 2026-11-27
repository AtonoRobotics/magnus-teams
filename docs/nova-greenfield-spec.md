# Nova Greenfield Theme & UX Overhaul — Engineering Specification

This document is the complete, self-contained specification for implementing the **Nova** flagship default theme and UI/UX overhaul in `magnus-teams`, paired with the interactive reference mockup at [`docs/nova-greenfield-mockup.html`](nova-greenfield-mockup.html).

---

## 1. Hard Architectural Rules (Non-Negotiable)

1. **Strict Theme Isolation**:
   - **No theme may depend on or bleed into another theme.**
   - None of `aurora.css`, `aurora-widgets.css`, `aurora-stage.css`, `aurora-rail.css`, or `aurora-compact.css` may contain `[data-style="nova"]`.
   - Nova owns its own standalone stylesheets: `nova.css`, `nova-stage.css`, `nova-shell.css` (each strictly **under 800 lines**).
   - **Never** paint an opaque `.shell::before` overlay over `<main>`. Nova's `<main>` is `position: relative; z-index: 1; background: transparent;` so the canvas, 3D orbs, and dissolving midnight halo render cleanly behind content without covering any UI.
2. **Default Theme**:
   - `Theme.DEFAULTS` in `desktop/ui/theme.js` MUST be:
     ```js
     { style: "nova", mode: "dark", accent: "blue", motion: "full", background: "particles", avatar: "pip" }
     ```
   - `STAGE_STYLES` includes `"nova"` so Nova uses the shared JS engines (`aurora.js` 3D floating orbs canvas, `aurora-avatar.js` companion mount, `aurora-stage.js` Side/Bottom Bento cards, `aurora-rail.js` compact rail/slide-out drawer) while keeping 100% independent CSS.
3. **Repo Constraints**:
   - Plain vanilla JS/CSS with zero build step (`desktop/ui/`).
   - Every new JS/CSS file in `desktop/ui/` MUST be registered in `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`.
   - Every new CSS file MUST also be added to `SHEETS` in `desktop/tests/stylesheet-check.mjs` and documented in `docs/ui-styles.md`.
   - Every source/CSS file MUST stay under **800 lines** (`ci/ratchet.sh size`).
   - Single-selector-per-property rule (`desktop/tests/stylesheet-check.mjs`): no duplicate selector+property across sheets, no undefined `var(--token)`, no unused CSS classes/IDs.
   - Accessibility floors: body text `>= 13px`, uppercase labels `>= 11px`, `44px` touch targets under `@media (any-pointer: coarse)`, and clean `@media (forced-colors: active)` fallback.
   - Never edit `tests/invariants/`.

---

## 2. Design Tokens (`desktop/ui/theme.js`)

### Dark Mode (`NOVA_DARK` — Default)
| Token | Value | Role |
|---|---|---|
| `--bg` / `--main` | `#0B0C0E` | Seamless pitch-carbon canvas |
| `--side` / `--panel` | `#141517` | Expanded left sidebar & right panel surface |
| `--surface` | `#17181B` | Matte cards, bento tiles, approval cards |
| `--raised` | `#1E2025` | Hover states, interactive surfaces |
| `--bubble` | `#1B1C1F` | Composer pill surface |
| `--seg` / `--seg-on` | `#17181B` / `#262932` | Segmented controls & active pills |
| `--fg` / `--fg2` / `--muted` | `#F1F3F8` / `#C2C8D6` / `#959DB0` | High-contrast ink hierarchy |
| `--line` / `--hair` / `--line-strong` | `#23262E` / `#1B1D23` / `#363A46` | Subtle hairline structure |
| `--shadow` | `0 18px 48px rgba(0,0,0,0.58)` | Modals, drawers, elevated menus |

### Light Mode (`NOVA_LIGHT`)
| Token | Value | Role |
|---|---|---|
| `--bg` / `--side` | `#F4F6FA` | Soft neutral rail/sidebar |
| `--main` | `#FFFFFF` | Clean white stage |
| `--surface` / `--raised` | `#F5F7FB` / `#EBF0F8` | Matte cards & hover |
| `--bubble` | `#F2F5FA` | Composer pill surface |
| `--fg` / `--fg2` / `--muted` | `#111827` / `#374151` / `#525D70` | Ink hierarchy |
| `--line` / `--hair` / `--line-strong` | `#E2E8F0` / `#EDF2F7` / `#CBD5E1` | Hairlines |

### Geometry (`STYLE_TOKENS.nova`)
- `--r-xs: 8px`, `--r-sm: 10px`, `--r-md: 14px`, `--r-lg: 18px`, `--r-xl: 24px`, `--r-pill: 999px`, `--r-send: 999px`, `--r-avatar: 50%`, `--r-composer: 24px`.

---

## 3. Living Companion Avatars & 3D Floating Orbs

1. **3D Perspective Floating Orbs**:
   - Enabled by default (`background: "particles"`).
   - Rendered via `aurora.js` canvas (`position: fixed; inset: 0; z-index: 0; pointer-events: none;`) behind the seamless `#0B0C0E` / `#FFFFFF` canvas.
2. **4 Selectable Living Companions** (`AVATAR_CHOICES` in `theme.js`, implemented in `nova-avatars.js` + `aurora-orb.js`, mounted via `aurora-avatar.js`):
   - `pip` (**Default**): Ceramic rounded-rect bot with antenna, expressive tracking eyes, and live state colors (`ready`, `listening`, `thinking`, `working`, `waiting`, `done`, `failed`).
   - `mochi`: Pearlescent organic spirit with reactive ears and blush/eye tracking.
   - `orb`: Magnus's liquid-glass luminous orb (`aurora-orb.js`).
   - `prism`: Faceted rotating star sprite.
3. **Live Companion Studio** (Settings → Appearance):
   - Interactive preview stage (`86px` live avatar) with state demo pills (`Ready`, `Listening`, `Thinking`, `Working`, `Waiting`, `Done`) and pointer tracking.

---

## 4. Screen-by-Screen Requirements (Match `docs/nova-greenfield-mockup.html`)

### 4.1 Left Navigation Rail & Expanded Sidebar (`app.js`, `nova-shell.css`)
- **Compact Rail (default on desktop)**: Transparent `60px` column with living companion at top, circular `40×40` borderless icon buttons (`border-radius: 999px`) for:
  1. New conversation (`#/chat/new`)
  2. Search (`⌘K`) & Slide-out Sidebar (`⌘B`)
  3. Inbox / Approvals (`#/inbox`) with live count badge (absolute top-right pill in compact mode)
  4. Projects (`#/projects`)
  5. Briefing & Routines (`#/briefing`)
  6. Explore (`#/explore`)
  7. Apps & Skills (`#/library`)
  8. Settings (`#/settings`) at bottom
- **Expanded Sidebar / Slide-out Drawer**:
  - Matte `#141517` (`--side`) with `1px solid var(--hair)` right border.
  - Whisper-quiet borderless nav rows (`min-height: 36px; border-radius: 10px`), rounded active highlight (`background: var(--seg-on)` — **no** vertical left-edge bar `::before`).
  - Hairline divider (`border-top: 1px solid var(--hair)`) separating Zone 1 (Primary Nav) from Zone 2 (Projects / Recent tree).
  - Hover-only delete/overflow controls (`.nav-conv-del`, `.nav-row-del` at `opacity: 0` until row hover/focus-within).
  - Indented project threads (`padding-left: 24px`), no default `<details>` triangle markers.
  - Matte rounded profile footer card (`.who`).

### 4.2 Top Bar (`conv-head` in `app.js`, `nova-ux.js`, `nova.css`)
- Subtle bottom hairline (`border-bottom: 1px solid var(--hair); padding: 10px 22px; background: transparent;`).
- Left: Breadcrumb (`Project / Conversation title`).
- Right: De-cluttered — tuck project-move `<select>` and `Delete conversation` inside a `•••` overflow menu (`<details class="conv-more">`). Include compact Nova quick controls (`Pip | Mochi | Orb | Prism` segmented pill + `Light / Dark` toggle pill on viewports `> 960px`), Notification bell, and Workspace Sidebar toggle.

### 4.3 Home / New Conversation (`#/chat/new`, `nova-ux.js`, `nova.css`)
- **Dissolving Midnight Cobalt Halo**: Fixed radial gradient behind the intro (`body[data-nova-intro="on"]::before`), fading smoothly to `opacity: 0` once messages appear.
- **Hidden Legacy Clutter**: Hide `.hello-banner`, `.hello-sub`, `.aurora-hello-chips`, `.hello-caps`, `.hero-tabs`, `.hello-cards`, `.hello-pillars`, and idle `.aurora-say-state` (`"Ready · standing by"`) on empty New Chat.
- **Exact Vertical Order** (centered vertically & horizontally in `section.conv` while `.conv-head` stays pinned at top):
  1. `84px` Living Companion + centered title (`34px`, `font-weight: 500`, `letter-spacing: -0.03em`)
  2. **Gemini Pill Composer** (`.composer-slot` → `.say` with `border-radius: 24px`, `background: var(--bubble)`, `border: 1px solid var(--hair)`, transparent `.composer-slot`, high-contrast circular monochrome Send button `background: var(--fg); color: var(--bg)`)
  3. **2-Column Editorial Showcase** (`.nova-showcase` directly below `.composer-slot`, `max-width: 820px`):
     - Left (`44%`): **Autonomous launchpads** (3 clickable one-tap workflow prompts)
     - Right (`56%`): Rotating `conic-gradient` **Prismatic Halo Card** containing:
       - Full-width bright white/slate top card (`linear-gradient(135deg, #f8fafc, #e2e8f0)`, `#0f172a` text, dark `Live Workspace` pill, 8-square color mosaic)
       - Bottom-left deep indigo card (`linear-gradient(135deg, #1e1b4b, #312e81)`) → `#/explore`
       - Bottom-right deep emerald card (`linear-gradient(135deg, #064e3b, #0f766e)`) → `#/library`

### 4.4 Active Conversation & Right Side Panel (`nova.css`, `nova-stage.css`, `nova-shell.css`)
- Airy prose (`.msg.magnus`: `15px / 1.64`, `max-width: 68ch`); matte rounded-18px user bubbles (`.msg.me`).
- **Right Side Panel (`.stage-artifact` / `aside.detail`)**:
  - Full-height matte `#141517` (`--side`) surface with `1px solid var(--hair)` left border (no floating glass card shadow).
  - Crisp `artifact-top` header with pill `Close` button (`border-radius: 999px`).
  - Pill-segmented `.panel-tabs` (`Activity | Tasks | Routines | Approvals | Files | Apps`) with `background: var(--surface)` container and `var(--seg-on)` active pill.
  - Matte `18px` rounded cards for `.gbox`, cleared/needs banners, active task cards, computer footer, and `<details>` sections; pill buttons inside the panel.
- **"While Magnus works" Tip Carousel** (`nova-ux.js`): Shown in `.stage-bento` during active runs when no approval is pending; dismissible per thread.
- **Verified Task Replay Scrubber** (`nova-ux.js`): Horizontal range slider injected above 2+ `.step` items on task views.

### 4.5 3-Tier Approvals (`asks.js`, `aurora-stage.js`, `nova.css`)
- **Tier 1 (Financial / Irreversible)**: `data-tier="financial"|"irreversible"`, `.ask-receipt-head` with crimson wash (`--bad`), dashed bottom divider, 2-column metadata tiles, inline **Edit before allowing**, and crimson primary confirm pill button.
- **Tier 2 (Batch Same-Kind)**: 1-click **Allow all N** bar (`.stage-batch-bar`, `data-stage-batch-approve` calling `Server.inbox.batch`) on the stage when 2+ non-financial approvals wait.
- **Tier 3 (Reversible Write)**: Compact left-accented card (`--warn`), progressive-disclosure decline note (`<details class="ask-decline-wrap">`) hidden until clicked.
- **Keyboard Triage (`nova-ux.js`)**: `A` = Allow, `E` = open Edit before allowing, `D` = open Decline note drawer & focus input.
- Must preserve `asks-check.mjs` invariants: `<p class="ask-sentence" data-ask-sentence>`, `<details class="ask-content" data-ask-content>`, exact button labels (`Allow`, `Allow always`, `Don't allow`), zero policy/rule/taint strings.

### 4.6 Morning Briefing & Quiet Routines (`#/briefing`, `nova-pages.js`, `nova.css`)
- Dedicated full-stage page showing executive headline, briefing summary, 3 metric cards (Pending Decisions, Quiet Routines, Active Tasks), and Quiet Routines list with **Dry run** (`{ dry_run: true }`) and **Run now** buttons.

### 4.7 Project Hub 2.0 (`#/projects` & `#/project/:id`, `projects.js`, `nova.css`)
- `#/projects`: 2-column matte card grid (`.project-list`) with hover-revealed delete button.
- `#/project/:id`: Gradient-tinted rounded-24px header, quiet inline hero composer (`data-project-ask` — nothing runs until submitted), and 2-column asymmetric workspace (`.project-hub-grid`: 62% Conversations + Files / 38% Instructions + Tasks + Routines).

### 4.8 Explore (`#/explore`), Apps & Skills (`#/library`), & Command Palette (`⌘K`)
- **Explore**: 3-column rounded-24px card grid (`.explore-grid`), collapsing to 2-col at `<= 960px` and 1-col at `<= 720px`.
- **Apps & Skills**: Featured Spotlight hero banner (`.nova-featured`) above a 2-column borderless horizontal catalog (`.lgrid`, `.lcard`).
- **Command Palette (`⌘K`)**: Matte rounded dialog with backdrop blur, including quick actions for Inbox/Approvals, Morning Briefing, Projects, Toggle Dark/Light, Appearance & Companion, and Approve all pending (when 2+ batchable approvals wait).

---

## 5. Verification Commands

Run locally before every commit:
```sh
for f in desktop/tests/*-check.mjs; do node "$f" || exit 1; done
sh ci/ratchet.sh size
cargo fmt --all --check
```
All 74 UI tests (including `stylesheet-check.mjs`, `nova-check.mjs`, `theme-check.mjs`, `preferences-check.mjs`, `asks-check.mjs`, `projects-flow-check.mjs`), size ratchet, and `cargo fmt` must pass with exit code `0`.
