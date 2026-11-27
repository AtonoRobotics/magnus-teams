# Actionable Morning Briefing, inline Trust Graduation, and rule capability picker

**Date:** 2026-11-27

## Decision
1. **Actionable Morning Briefing (`desktop/ui/nova-pages.js`):** Upgrade `#/briefing` from a KPI counter view into an actionable command surface with:
   - Inline pending approval cards (up to 3) + 1-click **Allow all N** batch approval right on the page.
   - Inline **Suggested standing rules** (Trust Graduation) with 1-click **Allow automatically**.
   - **Completed in the last 24 hours** tasks with verified receipt counts and **Open commitments** with 1-click **Mark done**.
   - Inline expandable **Dry run preview** card under routines showing the full prompt preview without starting a run.
2. **Inline Trust Graduation (`desktop/ui/app.js`):** After approving an ask (`approve:once` / `approve:task`), check `Server.rules.suggestions()` asynchronously and offer a non-blocking `"Allow automatically"` toast action when a repeated pattern is ready to graduate.
3. **Human-readable capability picker (`desktop/ui/rules.js`):** Add `#rule-actions-list` with plain-language labels (`Send email (Gmail)`, `Post message (Slack)`, etc.) on the rule form's Action input so users do not have to guess raw internal tool IDs.

## Reason
Users previously had to jump away from Briefing to Inbox to clear approvals, hunt in Settings to discover Trust Graduation suggestions, and type raw `gmail__send` identifiers to author rules manually. Verified by `desktop/tests/tier2-phase2-check.mjs`.
