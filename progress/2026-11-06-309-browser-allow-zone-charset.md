# Browser allow check restricts zone characters (2026-11-06)

Follow-up to the PR #308 review: zones with characters Node rejects (`fe80::1%eth_0`) now fail at load naming the entry, instead of passing validation and crashing the sidecar.
