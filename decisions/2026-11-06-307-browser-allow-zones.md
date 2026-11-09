# Browser allow check accepts zone-qualified IPv6 (2026-11-06)

- The allow-entry check tests IPv6 the way the sidecar does: exactly one `%` with a non-empty zone (`config.rs`, `is_ipv6`). Reason: review of PR #281 showed Node's isIPv6 accepts `fe80::1%eth0` (verified on Node 24, with the edge shapes: a second `%` or an empty zone do not parse, a colon inside the zone does), while Rust's parser takes no zone at all — so a config with a zone entry failed to load. Test: the new zone cases in `config::tests::a_bad_browser_allow_entry_fails_at_load_naming_itself` (fail on the direct parse).
