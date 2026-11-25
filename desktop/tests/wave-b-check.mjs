// Wave B regression checks: Edit-before-approving, Decline-with-note, Batch approvals,
// Staged Drafts UI, Trust Graduation suggestions + App picker + @domain wildcards,
// Visual Routine Creator/Editor with calendar-grade cadences, Dry run, and fixed Playbooks.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const read = f => readFileSync(join(root, f), "utf8");

const asks = read("desktop/ui/asks.js");
const app = read("desktop/ui/app.js");
const rules = read("desktop/ui/rules.js");
const ts = read("desktop/ui/tasks-schedules.js");
const discover = read("desktop/ui/discover.js");

// 1. Edit-before-approving & Decline-with-note.
assert.ok(asks.includes('data-ask-edit=') && asks.includes("Edit before allowing"), "asks.js renders collapsible Edit-before-allowing fields");
assert.ok(asks.includes('class="ask-decline-note"') && asks.includes('data-draft="rev-'), "asks.js renders optional decline note input");
assert.ok(app.includes("body.edited_input = edited"), "app.js respond() submits edited_input when fields were modified");

// 2. Batch approvals & Staged Drafts UI.
assert.ok(app.includes("data-batch-bar") && app.includes("data-batch-decide") && app.includes("Server.inbox.batch("), "needsView and wirePanel support batch approval/decline");
assert.ok(app.includes("function stagedDraftCard(") && app.includes("data-staged-act") && app.includes("Server.staged.resolve("), "needsView and wirePanel render and resolve Staged Drafts");

// 3. Trust Graduation & unified Guardrails UI.
assert.ok(rules.includes("Server.rules.suggestions") && rules.includes("data-trust-suggestions") && rules.includes("data-rule-graduate"), "rules.js loads and graduates repeated-approval suggestions");
assert.ok(rules.includes('id="rule-apps-list"') && rules.includes("@acme.com"), "rules.js offers connected app datalist and @domain wildcard help");
assert.ok(rules.includes("data-rules-grants") && rules.includes("Standing delegations"), "rules.js surfaces active standing delegations in plain English");

// 4. Calendar-grade schedule formatting, Visual Routine Creator, Dry run, and honest Playbooks.
assert.ok(app.includes("Weekdays at ") && app.includes("Monthly on day ") && app.includes("Once on "), "app.js rule() formats calendar-grade recurrences");
assert.ok(ts.includes("data-create-schedule") && ts.includes("scheduleCadenceFields") && ts.includes('data-act="dry-run-schedule"'), "tasks-schedules.js provides New routine creator, calendar cadence fields, and Dry run");
assert.ok(app.includes('case "dry-run-schedule":') && app.includes("dry_run: true"), "wirePanel handles schedule dry-run preview");
assert.ok(discover.includes('weekdays_at: "07:45"') && discover.includes('weekdays_at: "09:00"') && discover.includes('days: ["fri"], at: "16:00"'), "discover.js playbooks use real weekdays_at and weekly_at schedules");

console.log("wave-b check passed");
