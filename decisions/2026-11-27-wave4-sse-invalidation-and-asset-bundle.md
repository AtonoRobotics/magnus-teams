# Wave 4: Real-Time SSE Invalidation (`/v1/events`) & Zero-Build Rust Asset Bundling

## Decision
1. **Per-person SSE invalidation stream (`GET /v1/events`, `crates/magnus/src/api/events_sse.rs`, `kernel.rs`, `desktop/ui/{bridge,app,app-boot}.js`)**:
   - Added a bounded `tokio::sync::broadcast` channel (`INVALIDATION_CHANNEL_CAPACITY = 256`) on `Kernel` that emits compact `{ seq, domains }` frames from `Kernel::emit_all` whenever an event affecting `tasks`, `inbox`, `chat`, `schedules`, or `connectors` commits.
   - `GET /v1/events` authenticates the caller, filters frames to the caller's person (or org-wide events), sends 15s keepalives with session re-verification, and falls back cleanly on ring lag.
   - Wired `webBridge.events()` (`EventSource("/v1/events")`) in `desktop/ui/bridge.js`, `refreshDomains(domains)` in `desktop/ui/app.js`, and SSE-aware polling in `desktop/ui/app-boot.js` so healthy SSE connections skip the 10s 12-way full `refresh()` poll and immediately fall back to polling if disconnected.
2. **Zero-build Rust-concatenated UI bundles (`crates/magnus/src/ui/assets.rs`)**:
   - Parse `<link rel="stylesheet">` and `<script src="...">` order from `desktop/ui/index.html` once in a `LazyLock<Bundles>`, concatenate into `/app.bundle.css?v={v}` and `/app.bundle.js?v={v}`, pre-gzip both via `flate2`, and serve them with `Cache-Control: public, max-age=31536000, immutable`.
   - Keep `desktop/ui/` 100% plain JS/CSS with zero build step and keep all individual `/<file>` routes served intact.

## Reason
- Replaces 12-way idle polling with single-stream push invalidation (>95% reduction in idle polling traffic) and collapses 108 static asset HTTP requests on cold load into 2 immutable gzipped bundles (~280 KB total).
