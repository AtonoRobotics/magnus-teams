# Browser pages count their host as their source (2026-11-06)

Knowledge source counting now reads the host of `browser_navigate` results as it does `web_fetch`: two pages from one host are one source, not two.
