# Add Obsidian 2 and Studio themes

## Choice
Added two new themes alongside `obsidian` and `prism` in `desktop/ui/theme.js` and the desktop stylesheets:
- **Obsidian 2 (`obsidian-2`)**: Frames the main conversation/work pane (and split inspector) as a rounded pure-black `#000000` card inside a `#121215` matte sidebar shell, uses borderless `+ New conversation` and soft rounded active nav pills, adds colored file-type badges (`.fchip-inline` / `.fchip-tag` in `compose.js` and `chat.css`) for inline code filenames, and uses a circular send button.
- **Studio (`studio`)**: Adds a Google AI Studio-inspired `#181819` / `#141415` dark palette (and `#FFFFFF` / `#F1F3F4` light counterpart), pill-shaped sidebar nav and inspector tabs, a two-column editorial hero with a bento playbook grid on desktop (`chat.css`), and an elevated `22px` pill composer.

## Reason
The user asked to keep the existing `obsidian` theme intact and introduce both `Obsidian 2` and `Studio` as new selectable themes in Settings > Appearance, with every theme/mode/accent combination verified at >= 4.5:1 contrast by `desktop/tests/theme-check.mjs`.
