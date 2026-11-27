# 2026-11-27 — Wave 1A Frontend Performance Quick Wins

## Choice
1. **Retired `aurora-avatar.js` `setInterval` & boot `MutationObserver`**:
   - Replaced periodic `AVATAR_POLL_MS` polling and `documentElement` observation with reactive `UIStore.subscribe("*", queuePlace)`, `magnus-render`, and `DOMContentLoaded`.
2. **Retired duplicate `document.body` `MutationObserver` in `app-shell.js`**:
   - Replaced `wireDocument()`'s subtree observer with `magnus-render` + `AuroraCore.onDom` (`syncDocumentWidgets`).
3. **Added `ETag` / `If-None-Match` (`304 Not Modified`) caching in `bridge.js`**:
   - `webBridge.api()` caches `GET` ETags and returns cached parsed payloads on `304`, ready for Wave 2B's `seq`-based server ETags.

## Reason
- Removes unnecessary background timers and duplicate full-body DOM observers, and prepares the web client for zero-parse `304 Not Modified` polling.
