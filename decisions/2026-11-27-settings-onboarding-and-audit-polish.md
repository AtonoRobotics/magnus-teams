# 2026-11-27 — Settings Deduplication, Onboarding Precision & Audit Log Transparency (Tier 4 Phase 2)

## Choice
1. **Settings Deduplication (`settings-sections.js`)**:
   - Replaced the duplicated Relationship Notes, Standing Grants, and Commitments editors in `dataSection` with a concise summary card (`data-memory-summary`) showing live counts and direct links to `#/settings/memory` and `#/settings/rules`.
2. **Onboarding Precision (`onboarding.js`)**:
   - Updated Step 1 bundle badges to display `Connected (N/M)` when only a subset of Google/Microsoft sub-apps is connected.
   - Updated Step 2 approval simulator messages to state explicitly that the exercise is `"Simulation only — no real rule saved yet"`.
3. **Admin Audit Log Transparency (`admin-security.js`)**:
   - Added `AUDIT_ROWS_LIMIT = 200`, an explicit `"Showing latest 200 of N entries"` truncation banner (`auditMeta()`), and a `"Show all N"` toggle button that works both on initial load and after filtering.

## Reason
- Eliminates confusing duplicate Settings editors, sets accurate expectations during first-run onboarding, and prevents admins from missing older audit rows due to silent truncation.
