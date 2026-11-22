# User-named GitHub repo read origins in OpenWeb

**Choice.** When a user's original prompt literally names a GitHub `owner/repo` slug (without a full `https://...` URL), `web_reads::open_web_link_allowed` allows an `OpenWeb` worker (which holds zero private data, memory, or parent history) to issue read-only `GET` requests under three normalized HTTPS prefixes for that exact repo:
- `https://github.com/<owner>/<repo>/...`
- `https://api.github.com/repos/<owner>/<repo>/...`
- `https://raw.githubusercontent.com/<owner>/<repo>/...`

Paths containing `..`, `%2f`, or `%5c` are rejected before case-insensitive `owner/repo` prefix matching. Tainted `Private` runs remain strictly blocked from synthesizing URLs directly (they must delegate to `research_web`).

**Rate-limit fallback.** Unauthenticated `api.github.com` is capped at 60 requests/hour per server IP. When `api.github.com` returns 403/429, the worker should fall back to `raw.githubusercontent.com` / `github.com` HTML or prompt once to connect the GitHub connector.

**Reason.** Previously, a user asking Magnus to perform a read-only review of `AtonoRobotics/magnus-teams` without the GitHub connector attached hit fabricated-URL silent denial even inside `OpenWeb`, because `appears_in` only matched literal `http(s)://` URLs. Allowing exact user-named repo prefixes exclusively inside the data-free `OpenWeb` compartment fixes the failure with zero exfiltration risk.
