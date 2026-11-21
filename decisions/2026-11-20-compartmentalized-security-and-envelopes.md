# Compartmentalized security, lazy user-only envelopes, and draft-first staging

**Choice.** Replace sticky thread-level taint approval prompts for internet access and out-of-envelope writes with four kernel-enforced mechanisms:

1. **Compartmentalized workers (`crates/magnus/src/compartment/`):**
   - `OpenWeb` workers run with an ephemeral, cookie-free browser (`browser_*`, never `browser_as_*`) and zero access to private connectors, files, memory, or parent history.
   - `OpenWeb` spawned from a tainted `Private` run (`research_web`) is **link-following only** (may open only URLs from its own search results, loaded pages, or the user's original prompt—never URLs embedded in the query) and disables form typing/clicking (`browser_type`, `browser_select`, `browser_click`).
   - `web_search` runs directly in `Private` and shares a single per-run query gate (`compartment/query_gate.rs`) with `research_web`: always-on deterministic secret/PII scrubbing, a shared per-run byte budget (`MAX_TAINTED_QUERY_BYTES_PER_RUN`), a mandatory outgoing classifier whenever `private && untrusted` exist in context, and a denial retry cap (`MAX_QUERY_DENIALS_PER_RUN`).
   - Fabricated URLs in tainted `Private` runs are **silently denied** to the model with guidance to search first, rather than interrupting the user with an approval card.
   - Residual risk is stated plainly: the configured search provider sees bounded, classifier-checked queries, and link selection leaks a few bits per hop.

2. **Lazy, user-only typed envelopes (`crates/magnus/src/envelope/`):**
   - Typed `Destination` variants (`Email`, `EmailDomain`, `SlackChannel` by immutable ID, `GitHubRepo`, `Host`) prevent lookalike, case, plus-addressing, and renamed-channel bypasses.
   - Compiled lazily on the first outbound write attempt from **only the user's own messages** (never tool results or assistant turns) plus active grants. Contacts and the org directory only resolve names the user actually wrote; they never pre-authorize unmentioned contacts or the whole directory.
   - Write authority resets per user turn (and scopes to tasks and schedules), while web-egress guardrails stay active whenever private and untrusted content exist anywhere in the conversation context.
   - No `frozen` flag (only user actions construct or update an envelope) and no `max_spend_usd` (payments still ask every time).
   - Replying to an untrusted external sender while holding private data from outside that thread is deterministically staged as a draft.

3. **Draft-first staging (`crates/magnus/src/staging.rs`):**
   - Out-of-envelope stageable writes become native drafts (Gmail/Outlook), silent calendar events (`sendUpdates=none`), or Magnus-held staged items (Slack/GitHub/Linear/Asana) so runs do not block on modals.
   - Cards render from service read-back (`Shown`) and server-written sentences (`asks::sentence`) with destination provenance, never model-written summaries.
   - Sending a staged item offers fixed kernel scope steps (`once`, `recipient`, `domain`) written as standing grants, and `Ev::Staged(Change::Resolved)` tracks closure including external Gmail/Outlook draft send/delete reconciliation.
   - Unstageable actions (payments, destructive deletes, ACL/visibility changes, webhooks, `add_computer_destination`) and user/workspace `AskFirst` rules keep modal approval cards.

4. **Narrowed CaMeL Quarantined Reader (`crates/magnus/src/quarantine.rs`):**
   - Applied only to unattended schedules/routines whose envelope can reach someone other than their owner; owner-only routines run normally. Zero-tool, schema-constrained extraction strips undeclared keys and bounds strings (which remain untrusted text).

**Reason.** Sticky thread-wide booleans plus modal approvals for web lookups caused approval fatigue and security theater without stopping BEC/past-contact exfiltration. Structural compartmentalization, lazy user-only typed envelopes, and draft-first staging eliminate internet approval prompts while tightening the actual exfiltration boundary.

**Invariants (applied with owner approval).** Updated `a_fetch_cannot_carry_private_data_out_after_untrusted_input` in `tests/invariants/tests/inv4_taint.rs` for silent fabricated-URL denial, and added the 5 new compartmentalized security invariant tests in `tests/invariants/tests/inv4_compartments.rs` (keeping `inv4_taint.rs` under 800 lines).
