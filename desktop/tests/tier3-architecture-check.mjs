// Regression tests for Tier 3 Phase 4:
//   - aurora-stage.js decomposed below the 800-line ceiling via aurora-stage-view.js
//   - app.js file/verdict/glimpse helpers extracted into app-files.js
//   - nova-ux.js and nova-frame.js retired their global MutationObserver fallbacks
//     in favor of UIStore subscriptions and app.js's coalesced magnus-render hook
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");
const lines = rel => read(rel).split("\n").length;

// 1. aurora-stage.js is now strictly under the 800-line source ceiling
const stageLines = lines("desktop/ui/aurora-stage.js");
assert.ok(stageLines < 800, `aurora-stage.js must be < 800 lines (got ${stageLines})`);
assert.ok(lines("desktop/ui/aurora-stage-view.js") < 800, "aurora-stage-view.js must be < 800 lines");
assert.ok(lines("desktop/ui/app-files.js") < 800, "app-files.js must be < 800 lines");

// 2. nova-ux.js and nova-frame.js no longer instantiate MutationObserver
const novaUx = read("desktop/ui/nova-ux.js");
const novaFrame = read("desktop/ui/nova-frame.js");
assert.doesNotMatch(novaUx, /new MutationObserver/, "nova-ux.js must not instantiate a global MutationObserver");
assert.doesNotMatch(novaFrame, /new MutationObserver/, "nova-frame.js must not instantiate a global MutationObserver");
assert.match(novaUx, /magnus-render/, "nova-ux.js must listen for magnus-render");
assert.match(novaUx, /UIStore\.subscribe/, "nova-ux.js must subscribe to UIStore");
assert.match(novaFrame, /magnus-render/, "nova-frame.js must listen for magnus-render");
assert.match(novaFrame, /UIStore\.subscribe/, "nova-frame.js must subscribe to UIStore");

// 3. app.js emits coalesced magnus-render from paint()
const appJs = readAppBundle();
assert.match(appJs, /new CustomEvent\("magnus-render"\)/, "app.js paint() must emit coalesced magnus-render event");

// 4. Extracted modules registered in index.html and crates/magnus/src/ui/assets.rs
const indexHtml = read("desktop/ui/index.html");
assert.match(indexHtml, /<script src="aurora-stage-view\.js"><\/script>\s*<script src="aurora-stage\.js"><\/script>/);
assert.match(indexHtml, /<script src="app\.js"><\/script>\s*<script src="app-shell\.js"><\/script>\s*<script src="app-files\.js"><\/script>/);
const assetsRs = read("crates/magnus/src/ui/assets.rs");
assert.match(assetsRs, /aurora-stage-view\.js/);
assert.match(assetsRs, /app-files\.js/);

console.log(`tier3-architecture-check: ok (aurora-stage.js=${stageLines}L)`);
