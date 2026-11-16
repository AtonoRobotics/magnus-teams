// The page is painted before its scripts run, and WebView2 draws the window
// black wherever the page is not painted. The page's background has a solid
// fallback in both modes, the same colors the window starts with
// (`desktop/src-tauri/src/look.rs`), and nothing makes it clear.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../ui/app.css", import.meta.url), "utf8");
const look = readFileSync(new URL("../src-tauri/src/look.rs", import.meta.url), "utf8");
const rules = [...css.matchAll(/(^|[{}\n])\s*(html(?:\[[^\]]*\])?)\s*\{([^}]*)\}/g)].map(m => ({ sel: m[2], body: m[3] }));
const backgrounds = rules.filter(r => /background\s*:/.test(r.body));
const plain = backgrounds.filter(r => r.sel === "html");
assert.ok(plain.length >= 2, "html has no light and dark background of its own");
for (const r of plain) assert.match(r.body, /background\s*:\s*var\(--bg\s*,\s*#[0-9A-Fa-f]{6}\)/, `html's background has no solid fallback before the scripts run: ${r.body.trim()}`);
const window = name => { const m = new RegExp(`const ${name}: Color = Color\\(0x(..), 0x(..), 0x(..), 0xFF\\)`).exec(look); assert.ok(m, `${name} is missing from look.rs`); return `#${m[1]}${m[2]}${m[3]}`.toUpperCase(); };
const light = /(?:^|\n)\s*html\s*\{[^}]*var\(--bg\s*,\s*(#[0-9A-Fa-f]{6})\)/.exec(css)?.[1].toUpperCase();
const dark = /prefers-color-scheme:\s*dark\)\s*\{\s*html\s*\{\s*background\s*:\s*var\(--bg\s*,\s*(#[0-9A-Fa-f]{6})\)/.exec(css)?.[1].toUpperCase();
assert.equal(light, window("SOLID_LIGHT"), "the light fallback is not the window's light color");
assert.equal(dark, window("SOLID_DARK"), "the dark fallback is not the window's dark color");
assert.equal(backgrounds.filter(r => /transparent/.test(r.body)).length, 0, "html is never clear");
assert.ok(!/data-backdrop/.test(css), "no rule depends on a window material");

console.log("solid start ok");
