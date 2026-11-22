# Autonomous browser navigation via the outgoing URL gate

**Choice.** Replace the unconditional silent denial of model-constructed web/browser URLs in tainted (`private && untrusted`) runs with two structured checks:

1. **Classifier-gated URL check in `Private` runs (`compartment/query_gate.rs::check_web_url`, `boundary.rs`):**
   - Provenanced URLs (`plain_web_read`) still pass immediately with zero latency.
   - Model-constructed URLs pass through the same deterministic secret/PII scrubber, userinfo rejection, turn-scoped byte budget (`MAX_TAINTED_QUERY_BYTES_PER_RUN`), denial retry cap (`MAX_QUERY_DENIALS_PER_RUN`), and `outgoing_finding` classifier as `web_search` and `research_web`.
   - When the classifier clears the URL as carrying no private data (`UrlVerdict::Allow`), `browser_navigate` and `web_fetch` execute autonomously with zero approval prompts.
   - When scrubbing, budgets, or the classifier flag sensitive data (`UrlVerdict::Deny`), the call is silently denied.
   - When no classifier is configured or reachable, `browser_*` and queryless `web_fetch` fall back to the site-scoped envelope/approval gate (`UrlVerdict::Ask`, offering `Allow` / `Allow always` for the host or passing immediately if the host is already in the user's envelope), while query-bearing `web_fetch` URLs remain silently denied (preserving `inv4_taint.rs::a_fetch_cannot_carry_private_data_out_after_untrusted_input` and `inv4_compartments.rs::a_fabricated_url_in_turn_2_after_turn_1_taint_is_still_denied`).

2. **Direct clean HTTPS navigation in `OpenWeb` (`web_reads.rs::is_clean_open_web_url`):**
   - `OpenWeb` workers hold zero private data, zero parent history, and an ephemeral cookie-free browser, and their incoming task query has already passed `check_web_query`.
   - In addition to links from search results, loaded pages, the user's prompt, and user-named GitHub repos, `OpenWeb` workers may now navigate directly to clean public HTTPS URLs (domain host, no userinfo, no query string, no traversal/encoded slashes, passes `scrub_check`).

**Reason.** Hard-denying every `browser_navigate` whose URL was not literally typed by the user or returned verbatim by `web_search` broke autonomous browsing tasks—especially when `web_search` was unconfigured or when the agent needed to visit a known public site directly—and gave the user no way to approve the site when no classifier was configured. Routing constructed URLs through the existing `query_gate` preserves exfiltration protection while restoring autonomous browsing with zero unnecessary approval ceremony.
