# Stop Button in Chat Composer & Move Emergency Stop to Right Sidebar

## Changes
1. **Composer Stop Button** (`desktop/ui/chat-composer.js`, `desktop/ui/personal-chat.js`, `desktop/ui/chat.css`):
   - While a conversation has active running turns or tasks (`record.runningIds`) and the composer input is empty, the send arrow (`button.send`) transforms into a Stop button (`.is-stop`, square stop icon). Clicking it cancels the active run(s) via `Server.tasks.cancel(...)`.
2. **Removed Top-Bar Companion Pill** (`desktop/ui/app.js`, `desktop/ui/layout.css`):
   - Removed the `Companion` button (`.inspector-pill`) from `convHead()` and its unused CSS rules from `desktop/ui/layout.css`.
3. **Moved Emergency Stop to Right Sidebar** (`desktop/ui/app.js`, `desktop/ui/layout.css`, `desktop/tests/admin-check.mjs`, `desktop/tests/killswitch.mjs`):
   - Moved `[data-admin-kill]` from `#workspace-bar` into `#detail` (`companionHeader()` and sub-panel `.dh-top`), calling `showKillControl()` when `#detail` is rendered for admins. This also allows `#workspace-bar` to stay collapsed in desktop conversation view.

## Verification
All desktop UI tests (`theme-check.mjs`, `preferences-check.mjs`, `companion-panel-check.mjs`, `discover-check.mjs`, `admin-check.mjs`, `stylesheet-check.mjs`, `globals-check.mjs`, `wording-check.mjs`, `chat-view-check.mjs`, `onboarding-check.mjs`) and `ci/ratchet.sh size` pass.
