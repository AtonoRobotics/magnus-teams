# Remove link-following-only URL restrictions on OpenWeb workers

**Choice.** Remove the `link_following_only` URL denial gate on `OpenWeb` workers (`boundary.rs`, `web_reads.rs`) and allow `browser_click` in all `OpenWeb` runs (`compartment/mod.rs`).

**Reason.** `OpenWeb` workers are structurally isolated with zero private data, zero private connectors, zero memory, zero parent history, and an ephemeral cookie-free browser. Dragging taint/URL-provenance restrictions into `OpenWeb` recreated Rule-of-Two theater inside a clean compartment and broke autonomous web browsing. Security holds at the single handoff boundary (`Private` → `OpenWeb` query via `check_web_query`), not by policing URLs inside a worker that holds nothing private.
