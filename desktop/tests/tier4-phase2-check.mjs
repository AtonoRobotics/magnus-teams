// Regression tests for Tier 4 Phase 2:
//   - Settings dataSection deduplicates Relationship Notes, Commitments & Standing Delegations
//   - Onboarding Step 1 shows partial Connected (N/M) counts & Step 2 clarifies simulation-only
//   - Admin Security audit log displays truncation count + Show all toggle
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. Settings deduplication
const settings = read("desktop/ui/settings-sections.js");
assert.match(settings, /data-memory-summary/, "dataSection renders concise memory/rules summary card");
assert.match(settings, /href="#\/settings\/memory"/, "dataSection links to Memory & Context");
assert.match(settings, /href="#\/settings\/rules"/, "dataSection links to Rules");

// 2. Onboarding precision
const onboarding = read("desktop/ui/onboarding.js");
assert.match(onboarding, /Connected \(\$\{p\.connected\}\/\$\{p\.apps\.length\}\)/, "Step 1 shows Connected (N/M) for partially connected bundles");
assert.match(onboarding, /Simulation only — no real rule saved yet/, "Step 2 simulator clarifies no real rule is saved");

// 3. Admin audit log truncation visibility + Show all toggle
const sec = read("desktop/ui/admin-security.js");
assert.match(sec, /const AUDIT_ROWS_LIMIT = 200;/, "audit row cap is a named constant");
assert.match(sec, /Showing latest \$\{AUDIT_ROWS_LIMIT\} of \$\{rows\.length\} entries\./, "auditMeta shows truncation count");
assert.match(sec, /data-audit-show-all/, "auditMeta renders Show all toggle button");

console.log("tier4-phase2-check: ok");
