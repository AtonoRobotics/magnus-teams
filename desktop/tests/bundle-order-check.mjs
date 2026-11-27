// The app's scripts run as one bundle (crates/magnus/src/ui/assets.rs joins
// every <script src> of index.html, in order, with "\n;\n"). One script
// behaves differently from many in two ways, and either stops the whole page:
// - a top-level `const`/`let`/`class` is in its temporal dead zone until its
//   line runs, so an earlier top-level `typeof NAME` throws instead of giving
//   "undefined" (test.401: chat-composer.js asked `typeof UI` before
//   app-shell.js declared it, and the page never signed in);
// - a name declared twice at the top level is a syntax error for the whole
//   bundle, not just for the second file.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const html = readFileSync(join(ui, "index.html"), "utf8");
const files = [...html.matchAll(/^\s*<script src="([^"]+\.js)"><\/script>\s*$/gm)].map(m => m[1]);
assert.ok(files.length > 20, `index.html lists ${files.length} scripts`);

/** Each line of the bundle with the file it came from. */
const lines = files.flatMap(f => readFileSync(join(ui, f), "utf8").split("\n").map((text, n) => ({ f, n: n + 1, text })));
const bundle = files.map(f => readFileSync(join(ui, f), "utf8")).join("\n;\n");

// It parses as one script.
try { new vm.Script(bundle, { filename: "app.bundle.js" }); }
catch (e) { assert.fail(`the bundle does not parse: ${/** @type {Error} */ (e).message}`); }

/** @type {Map<string, number>} where each top-level lexical name is declared */
const declared = new Map();
lines.forEach((l, i) => {
  const m = l.text.match(/^(?:const|let|class)\s+([A-Za-z_$][\w$]*)/);
  if (m && !declared.has(m[1])) declared.set(m[1], i);
});

const problems = [];
lines.forEach((l, i) => {
  // A line at column 0 is top-level code that runs as the bundle loads.
  if (/^\s|^\/\/|^\*|^\/\*/.test(l.text)) return;
  for (const m of l.text.matchAll(/typeof\s+([A-Za-z_$][\w$]*)/g)) {
    // Inside a function on the same line, it runs later, once all is declared.
    if (/=>|\bfunction\b/.test(l.text.slice(0, m.index))) continue;
    const at = declared.get(m[1]);
    if (at !== undefined && at > i) problems.push(`${l.f}:${l.n} asks typeof ${m[1]} before ${lines[at].f}:${lines[at].n} declares it`);
  }
});
assert.deepEqual(problems, [], "top-level code reads a name before the bundle declares it");
console.log(`ok - ${files.length} scripts run as one bundle`);
