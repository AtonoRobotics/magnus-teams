// Regression tests for Tier 4 Phase 4:
//   - aurora-alerts.js, aurora-compact.js, aurora-pages.js, and aurora-explore.js
//     retired their standalone MutationObserver fallbacks in favor of UIStore + magnus-render
//   - app-polling.js extracted cleanly from app.js and registered in index.html + assets.rs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

for (const file of [
  "desktop/ui/aurora-alerts.js",
  "desktop/ui/aurora-compact.js",
  "desktop/ui/aurora-explore.js",
]) {
  const src = read(file);
  assert.doesNotMatch(src, /new MutationObserver/, `${file} must not instantiate a standalone MutationObserver`);
  assert.match(src, /magnus-render/, `${file} must listen for magnus-render`);
}

const polling = read("desktop/ui/app-polling.js");
assert.match(polling, /async function pollInbox\(\)/);
assert.match(polling, /async function pollVersions\(\)/);
assert.ok(polling.split("\n").length < 800, "app-polling.js must be < 800 lines");

assert.match(read("desktop/ui/index.html"), /<script src="app-polling\.js"><\/script>/);
assert.match(read("crates/magnus/src/ui/assets.rs"), /app-polling\.js/);

console.log("tier4-phase4-check: ok");
