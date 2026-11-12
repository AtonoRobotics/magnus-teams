// The page is painted before its scripts run: the window on Windows can show
// the system's glass, and WebView2 draws it black wherever the page is not
// painted. The page's background has a solid fallback in both modes, and only
// the system's own material (data-backdrop="native") makes it clear.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../ui/app.css", import.meta.url), "utf8");
const rules = [...css.matchAll(/(^|[{}\n])\s*(html(?:\[[^\]]*\])?)\s*\{([^}]*)\}/g)].map(m => ({ sel: m[2], body: m[3] }));
const backgrounds = rules.filter(r => /background\s*:/.test(r.body));
const solid = /background\s*:\s*var\(--backdrop\s*,\s*#[0-9A-Fa-f]{6}\)/;
const plain = backgrounds.filter(r => r.sel === "html");
assert.ok(plain.length >= 2, "html has no light and dark background of its own");
for (const r of plain) assert.match(r.body, solid, `html's background has no solid fallback before the scripts run: ${r.body.trim()}`);
assert.ok(/prefers-color-scheme:\s*dark\)\s*\{\s*html\s*\{\s*background\s*:\s*var\(--backdrop\s*,\s*#161618\)/.test(css), "the dark fallback is missing");
for (const r of backgrounds.filter(r => /transparent/.test(r.body))) assert.equal(r.sel, 'html[data-backdrop="native"]', "html is clear without the system's material");

console.log("solid start ok");
