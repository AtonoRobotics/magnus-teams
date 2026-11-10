// The page's scripts share one global scope: a top-level name declared in
// two of them is one name. A second `function` of the same name silently
// replaces the first everywhere, as team-panel.js's taskCard (cc2eb4b)
// replaced the conversation's: a chat's task cards lost their link, status
// and Cancel. Each top-level name is declared once across the scripts
// index.html loads.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const page = readFileSync(join(ui, "index.html"), "utf8");
const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.includes("app.js") && scripts.includes("team-panel.js"), `index.html loads the page's scripts: ${scripts}`);

/** @type {Map<string, string[]>} */
const where = new Map();
for (const file of scripts) {
  for (const line of readFileSync(join(ui, file), "utf8").split("\n")) {
    const m = line.match(/^(?:async\s+)?(?:function\*?|const|let|var|class)\s+([A-Za-z_$][\w$]*)/);
    if (m) where.set(m[1], [...(where.get(m[1]) || []), file]);
  }
}
const twice = [...where].filter(([, files]) => files.length > 1).map(([name, files]) => `${name} (${files.join(", ")})`);
assert.deepEqual(twice, [], `declared at the top level of more than one script: ${twice.join("; ")}`);
console.log(`globals check passed: ${where.size} names across ${scripts.length} scripts`);
