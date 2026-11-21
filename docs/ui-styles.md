# Changing how the app looks

The desktop app and the browser page share one UI in `desktop/ui`: plain
HTML, JavaScript and CSS with no build step. This page says where each
kind of style lives, so a change goes in one place and shows.

## Where a style lives

| What you change | Where |
|---|---|
| A colour, in either theme or for an accent | `theme.js`: the `LIGHT`, `DARK` and `ACCENTS` tokens. Sheets read them as `var(--name)` |
| Fonts, base elements (buttons, inputs, links, tables, dialogs), focus rings, motion, high-contrast mode | `base.css` |
| The window's frame: sidebar, top bar, the panel beside a conversation, the splitter, banners and toasts, and how they rearrange at each width | `layout.css` |
| A conversation: its head, messages, the composer, the step timeline, results, task and schedule cards, questions and approvals | `chat.css` |
| The other pages: Library and connectors, Settings and Appearance, Files and projects, Activity, the Admin cards | `pages.css` |
| A team workspace | `teams.css` |
| The computer panel | `computer.css` |
| The Admin section's own parts | `admin.css` |
| Sign-in and first run | `onboarding.css` |
| Aurora and Obsidian 2 Aurora: the frame, ambient canvas and motion | `aurora.css` |
| Aurora's widgets: request and approval cards, pill buttons, an answer's reasoning pill, the composer's state line and glow and what it is told, a decision's line in the thread, decisions confirmed in place, toasts as they go | `aurora-widgets.css` |
| Aurora's stage: the one frame the conversation and the canvas share, the boxes above the input (decisions with their answers, messages among them), the artifact canvas, and Explore as the mockup's bento | `aurora-stage.css` |
| Aurora's rail: the navigation as icons, the full sidebar sliding in over the stage when asked for, and pinned beside it; the avatar's button | `aurora-rail.css` |

`index.html` loads the sheets in that order, and later wins a tie. The
server serves each of them (`crates/magnus/src/ui/assets.rs`), and the
installed app bundles them, so an app shows a change only after it
updates.

## Rules

`desktop/tests/stylesheet-check.mjs` checks each of these on every push.

- **Write a selector once.** Find the component's rule and change it there.
  A second rule for the same selector further down is how changes stopped
  showing: the later rule won and the edit above it did nothing.
- **Put a variant next to its rule.** A width, pointer or theme variant
  goes in a media block right after the component's own rule, or in the
  sheet's responsive section for that area.
- **Set nothing that is always overridden.** If a later rule for the same
  selectors sets the property, change that rule instead.
- **Use tokens that exist.** A `var(--name)` with no definition drops
  the whole declaration, and the browser reports no error. Add the token
  to `theme.js` in both themes, or give the `var()` a fallback.
- **Delete a style with its markup.** A rule for a class or id that no
  code uses fails the check.
- **Keep a sheet under 800 lines.** Split a large area into another sheet
  and add it to `index.html`, `assets.rs` and the check's list.

## Floors that stay

Some values are accessibility floors. A compact design does not go below
them:

- Text people read stays at 13px or more. Uppercase labels stay at 11px or more.
- Anything you click or tap is at least 44px on a touch screen
  (`@media (any-pointer: coarse)`). A smaller size applies only with a
  mouse (`@media (pointer: fine)`).
- Inputs and the composer keep a visible boundary (`--line-strong`).
- Every control shows the focus ring (`--focus`) when reached by keyboard.
- High-contrast mode (`@media (forced-colors: active)`) keeps system
  colours.

## Checking a change

`node desktop/tests/stylesheet-check.mjs` takes a second. The screen
tests in `desktop/tests` (`looks.mjs`, `app-looks.mjs`,
`workspace-layout.mjs`) check readability and layout in both themes.
