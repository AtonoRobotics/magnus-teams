# A search through a common search engine is a search, and asks no one

**Choice.** Opening a search page on Google, Bing, DuckDuckGo or Brave (https, the engine's own search path, a `q` parameter) counts as a search, as `web_search` does: it carries nothing out (`web_reads::is_search`). A look-alike host, plain http, a redirect through the engine (`/url?q=`) or a page with no query is not one.

**Reason.** The spec: "Reads, and everything inside the computer, are not boundary actions and pass without review." A search is a read, and the earlier decision (2026-11-12-web-reads-by-provenance) already lets a search go to its provider, which no page chooses. The production server has no search provider configured, so the agent searched by opening an engine's address, which it had written itself. After the work had read private data, each such search asked (owner, 2026-11-13: "Every web search seems to require approval").

**Still open.** Configuring a search provider (`[web] search_url`) gives the agent its `web_search` tool. That needs an account with a search service, which is the owner's to choose.
