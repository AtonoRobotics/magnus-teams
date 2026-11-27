# 2026-11-27 — Per-Session Autonomy Leash and Editable Pre-Execution Plan Surface

## Choice
1. **Per-Session Autonomy Leash (`desktop/ui/app.js`, `desktop/ui/personal-chat.js`)**:
   - Added a persisted `<select data-autonomy>` control in `modelPicker()` (wrapped in `<label class="small">` so it remains visible beside `.model-chip` in compact Aurora/Nova composers) with four strictly-tightening modes:
     - `standard`: *Ask on external & money* (workspace policy default)
     - `cautious`: *Ask on every write*
     - `draft_only`: *Draft & read only*
     - `plan_first`: *Review plan first*
   - In `personal-chat.js`, selecting `cautious` or `draft_only` prepends an explicit session leash instruction and attaches `body.autonomy_mode` when non-standard. It never loosens workspace policy, org rules, or admin kill switches.
2. **Editable Pre-Execution Plan Surface (`desktop/ui/plan-surface.js`)**:
   - Added a standalone `PlanSurface` module (registered in `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`) invoked automatically when `autonomy === "plan_first"`.
   - Parses plain-language goals into editable discrete steps, highlights steps touching external, destructive, or financial verbs with a `Will ask` gate chip, and lets the user edit, reorder (`↑`/`↓`), delete (`×`), or add (`+ Add step`) steps before clicking `Start plan` (or `Cancel` to abort without starting a run).

## Reason
- Users often want a stricter leash for a single sensitive task (e.g., drafting only or reviewing the exact plan before execution) without editing persistent workspace rules.
- Keeping the leash strictly tightening preserves the workspace security invariants while giving immediate per-run control directly in the composer.
- Isolating the modal editor in `plan-surface.js` keeps `personal-chat.js` and `app.js` concise and within the 800-line file size ratchet.
