# Aurora's navigation is a rail

## Choice

Step C of the Aurora composition (`2026-11-21-aurora-composition.md`). In
Aurora and Obsidian 2 Aurora, on a window wide enough for a sidebar:

- **The navigation is a rail**, 64px of icons: the avatar, then New
  conversation, Search, Conversations and Explore, with Settings at its foot.
  Trash and the projects and conversations tree are in the full sidebar, not
  on the rail.
- **The full sidebar slides in over the stage** when the person asks for it:
  the Conversations button, or Ctrl+B (⌘B). Never on hover: a sidebar that
  opens under a passing pointer covers the conversation the person was
  reading. The stage does not move under it, and is dimmed lightly.
- **It closes** when a conversation or a place in it is picked (which then
  opens), on Esc, on a click outside it, and on its Close. Closed by Esc,
  Close or a click outside, the focus goes back to Conversations.
- **Its pin keeps it open** beside the stage as a sidebar, through a reload,
  until it is unpinned. This is a new preference, `sidebar` (`rail`, the
  default, or `pinned`), which `Theme.navigation` turns into the layout's
  navigation. Other styles keep the navigation preference they had
  (expanded or compact) and their collapse control; Aurora's rail ignores it,
  so nobody who chose an expanded sidebar elsewhere starts Aurora with one
  pinned they did not ask for.
- **Its rows take the room their words need** on a mouse: 28px conversation
  rows with 2px between them, and tighter gaps between its groups. A touch
  screen keeps its 44px targets.
- A narrow window keeps the drawer it had, with no pin.

The slide-out is app.js's navigation drawer, the modal dialog the narrow
window already uses, so it brings that dialog's focus handling, Esc and
outside click with it. `aurora-rail.js` opens it from the rail, draws the
full sidebar in it rather than the rail's icons, and gives the focus back;
`aurora-rail.css` draws the rail and the slide-out. app.js changes in one
place: the layout asks `Theme.navigation(prefs)` for the navigation, where it
read `prefs.navigation`.

## Reason

The owner: "Can the side bar slide out only when the user wants to it's
contents? Is there a better way?", and "Reduce the space that is currently in
the side panel lines". What needs the person now arrives on the stage
(`2026-11-21-aurora-alerts.md`), so the sidebar only navigates: it need not
hold room all the time. A rail keeps the few things reached for often one
click away, and the rest a click or a shortcut away, without the stage
giving up 228px for a list read now and then.

## Verification

`desktop/tests/aurora-rail.mjs`, run by the `workspace_layout` scratch test
against a real server: the rail's contents and order, the slide-out over a
stage that stays, Esc with the focus back, Ctrl+B, a click outside, a pick
that opens, the pin through a reload and unpinned, the rows' height, another
style unchanged, and the narrow window's drawer. It fails before this change
(there is no Conversations button). `desktop/tests/preferences-check.mjs`
checks the new preference and `Theme.navigation`.
