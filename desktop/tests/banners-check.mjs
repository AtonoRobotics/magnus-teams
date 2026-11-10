// The notices above a conversation (2026-11-10): in a wide window the
// "Updates since your last session" notice drew as a column one word wide
// and could not be closed. #banners was capped at 760px while its padding
// was sized from the whole pane, so the padding ate its width. A capped
// banner's padding is fixed, and every notice can be closed.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const css = readFileSync(join(ui, "app.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const js = readFileSync(join(ui, "app.js"), "utf8");

const rules = [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)].filter(([, sel]) => sel.split(",").some(s => s.includes("#banners")));
assert.ok(rules.length > 0, "app.css styles #banners");
for (const [, sel, body] of rules) {
  assert.ok(!/padding[^;]*100%/.test(body), `#banners gets padding sized from the pane, which a capped column cannot hold: ${sel.trim()} { ${body.trim()} }`);
}
assert.ok(rules.some(([, , body]) => /max-width:\s*728px/.test(body)), "#banners keeps the composer's 728px column");

const updates = js.match(/Updates since your last session\.[^`]*?<\/div>/);
assert.ok(updates, "app.js draws the updates notice");
assert.match(updates[0], /data-act="dismiss-updates"/, "the updates notice has a Dismiss button");
assert.match(js, /closest\("\[data-act=dismiss-updates\][^"]*"\)\)\s*\{\s*App\.feedDismissed = true;\s*banners\(\);/, "Dismiss closes the notice");
assert.match(js, /const sessionUpdates = !App\.feedDismissed &&/, "a dismissed notice stays closed");
assert.match(js, /App\.feed = await api\("POST", "\/v1\/home\/opened"[^\n]*\n\s*App\.feedDismissed = false;/, "a new session shows its updates again");
console.log("banners check passed");
