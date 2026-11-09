# Browser pages count their host as their source (2026-11-06)

- `source_of` treats `browser_navigate` as it treats `web_fetch`: the URL's host is the source (`knowledge.rs`). Reason: it named `browser_open`, a tool that does not exist, so pages read in the browser were not told apart by host and two pages of one site counted as independent sources. Test: `knowledge::tests::two_browser_pages_from_one_host_count_as_one_source` (fails on the old name: both pages read back as `tool:browser_navigate`).
