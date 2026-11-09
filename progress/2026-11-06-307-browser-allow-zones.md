# Browser allow check accepts zone-qualified IPv6 (2026-11-06)

Follow-up to the PR #281 review: `[browser] allow` entries with a zone (`fe80::1%eth0`) load as the sidecar reads them, instead of failing the appliance at start.
