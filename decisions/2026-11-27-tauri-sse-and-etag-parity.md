# Tauri Desktop App Parity: `/v1/events` SSE Invalidation & `ETag` `304` Caching

## Decision
1. Added [`desktop/src-tauri/src/events_sse.rs`](../desktop/src-tauri/src/events_sse.rs) (`events_start`, `events_stop`, `stop()`) to relay `/v1/events` SSE invalidation frames and connection state (`events`, `events-state`) to the desktop WebView, stopping cleanly on `sign_out`.
2. Extended Tauri's `api` command in [`desktop/src-tauri/src/lib.rs`](../desktop/src-tauri/src/lib.rs) to accept `etag: Option<String>`, forward `If-None-Match`, and return the server's `ETag` header.
3. Added `_etags`, `clearCache()`, `304 Not Modified` handling, and `events()` to `appBridge` in [`desktop/ui/bridge.js`](../desktop/ui/bridge.js), clearing `_etags` on `signIn`, `providerSignIn`, `signOut`, `401`, and `Resources.reset()`.

## Reason
Brings the native Tauri desktop app to full parity with the web app so desktop sessions get the same SSE push invalidation and `304 Not Modified` traffic reduction.
