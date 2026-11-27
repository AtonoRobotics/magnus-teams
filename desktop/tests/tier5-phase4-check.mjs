// Regression tests for Tier 5 Phase 4:
//   - aurora-reasoning.js and aurora.js retired their fallback MutationObservers
//   - app-library.js and app-settings.js extracted cleanly (<800L each) and
//     registered in index.html + crates/magnus/src/ui/assets.rs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");
const lines = rel => read(rel).split("\n").length;

// 1. Final Aurora fallback MutationObservers retired
const reasoning = read("desktop/ui/aurora-reasoning.js");
assert.doesNotMatch(reasoning, /new MutationObserver/, "aurora-reasoning.js must not instantiate a MutationObserver");
assert.match(reasoning, /magnus-render/, "aurora-reasoning.js listens for magnus-render");

const aurora = read("desktop/ui/aurora.js");
assert.doesNotMatch(aurora, /new MutationObserver\(handleDom\)/, "aurora.js must not instantiate fallback handleDom MutationObserver");
assert.match(aurora, /magnus-render/, "aurora.js listens for magnus-render");

// 2. Extracted modules and app.js are all strictly < 800 lines and registered
for (const m of ["app.js", "app-shell.js", "app-files.js", "app-polling.js", "app-chat.js", "app-inbox.js", "app-panel.js", "app-library.js", "app-settings.js", "app-boot.js"]) {
  assert.ok(lines(`desktop/ui/${m}`) < 800, `${m} must be < 800 lines (got ${lines(`desktop/ui/${m}`)})`);
}

const indexHtml = read("desktop/ui/index.html");
assert.match(indexHtml, /<script src="app-shell\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-chat\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-inbox\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-panel\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-library\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-settings\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-boot\.js"><\/script>/);
const assetsRs = read("crates/magnus/src/ui/assets.rs");
for (const m of ["app-shell.js", "app-chat.js", "app-inbox.js", "app-panel.js", "app-library.js", "app-settings.js", "app-boot.js"]) {
  assert.match(assetsRs, new RegExp(m.replace(".", "\\.")));
}

console.log(`tier5-phase4-check: ok (app.js=${lines("desktop/ui/app.js")}L, app-library.js=${lines("desktop/ui/app-library.js")}L, app-settings.js=${lines("desktop/ui/app-settings.js")}L)`);
