// Regression tests for Tier 4 Phase 1:
//   - High-risk Allow always confirmation on irreversible & financial asks
//   - Edited ask parameters stay one-time (never promoted to standing rules)
//   - Outside-agent one-time inbound_key Copy, Clear, and auto-clear onLeave
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

const asks = read("desktop/ui/asks.js");
assert.match(asks, /Edits apply to this single action only\./, "askEditFields clarifies one-time scope");
assert.match(asks, /o === "allow_always" && highRisk[\s\S]*?data-risk="\$\{esc\(i\.tier\)\}"/, "high-risk allow_always buttons carry data-risk");

const app = readAppBundle();
assert.match(app, /if \(scope === "always" && body\.edited_input\)\s*\{\s*scope = "once";\s*body\.scope = "once";/, "edited_input downgrades allow_always to once");
assert.match(app, /if \(risk === "irreversible" \|\| risk === "financial"\)\s*\{\s*const ok = await ask\(/, "high-risk allow_always requires explicit confirmation modal");

const oa = read("desktop/ui/outside-agents.js");
assert.match(oa, /data-act="outside-copy-key"/, "outside-agents renders Copy key button");
assert.match(oa, /data-act="outside-clear-key"/, "outside-agents renders Clear button");
assert.match(oa, /onLeave\(\(\) => \{\s*OutsideAgents\.key = null;\s*\}\)/, "outside-agents clears inbound_key on route leave");

console.log("tier4-phase1-check: ok");
