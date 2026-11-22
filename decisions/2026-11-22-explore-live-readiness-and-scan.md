# Explore: live idea scanning, app-aware readiness, and inline 1-click actions

**Choice.** Upgrade Explore (`desktop/ui/discover.js`, `desktop/ui/pages.css`) and add `POST /v1/explore/scan` (`crates/magnus/src/openers.rs::scan_for_ideas`, `crates/magnus/src/api.rs`, `desktop/ui/server.js`):

1. **On-demand read-only idea scanning (`POST /v1/explore/scan`):** Explore's hero replaces the duplicate scripted discovery modal button with a real **Scan for new ideas** action that launches a background `Review` run (no conversation opened; suggestions appear in Explore's **For you**; deduplicated while a review scan is already running or while the kill switch is active).
2. **App-aware readiness & active routine detection:** Playbooks cross-reference `/v1/connectors` and `App.schedules` so ready playbooks rank first, active routines display an `Active` chip with a direct link to `#/schedules`, and playbooks missing a required app offer a direct `Connect <App>` link to `#/library/<app>` instead of a dead-end prompt.
3. **Inline 1-click configure / schedule & truthful guardrail badges:** Playbooks with placeholders (`[owner/repo]`, `[my space]`, `[channel or topic]`) or schedules open a compact inline form right on the card to fill inputs or set a daily time and launch via `Server.schedules.create` / `Server.chat.send`. Each card shows its actual security guardrail (*Read-only*, *Drafts first*, *Isolated public browser*).
4. **Flicker-free incremental grid updates:** Filtering or typing in `#explore-search` replaces only `.explore-grid` (preserving search focus, `For you` cards, and `AuroraExplore`'s `MutationObserver` FLIP transitions).

**Reason.** Explore was previously a static 9-card brochure that ignored connected apps and active schedules, dumped raw bracketed prompts into chat, repainted the whole page on every keystroke, and gave users no way to refresh Magnus's live **For you** ideas after cold start.
