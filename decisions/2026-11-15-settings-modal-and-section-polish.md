# Settings Dismissible Popup Modal and Section Theme Alignment

## Context
The owner requested that Settings open as a popup modal that disappears when finished instead of navigating away from the active conversation, and that Apps, Files, and Admin be aligned with the Obsidian hairline theme.

## Decision
- Added `openSettingsModal(which)` in `desktop/ui/app.js` backed by `<dialog id="settings-modal" class="settings-modal">`, triggered directly when clicking the sidebar Settings link (without changing the active conversation) and via `#/settings/*` routes.
- Added `Done`, `Esc`, and backdrop-click dismissal to close the modal cleanly.
- Styled `.settings-modal`, Apps cards (`.lcard`, `.ctile`, `.lseg`), Files rows (`.file-row`), and Admin pill tabs/tables (`.admin-sub`, `.admin-cards .card`, `.admin-section table.t`) in `desktop/ui/modern-ux.css`.

## Reason
Keeps the user's conversation anchored while adjusting settings, and unifies Apps, Files, and Admin under the same Obsidian hairline visual grammar.
