// Architectural guardrails for strict theme isolation and Nova-as-default:
// 1. Base CSS sheets contain zero [data-style=...] selectors.
// 2. Each theme-specific stylesheet only targets its own data-style value(s).
// 3. Theme.DEFAULTS.style is "nova" and every style in Theme.STYLES is registered
//    in Theme.THEMES with palettes, radii, defaultAccent, and declarative features.
// 4. Shared JS modules query Theme.has()/Theme.get() instead of hardcoding `=== "nova"`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const stripComments = s => s.replace(/\/\*[\s\S]*?\*\//g, "");

// 1. Base stylesheets must be 100% theme-agnostic.
const BASE_SHEETS = ["base.css", "layout.css", "chat.css", "pages.css", "teams.css", "computer.css", "admin.css", "onboarding.css"];
for (const sheet of BASE_SHEETS) {
  const css = stripComments(ui(sheet));
  assert.doesNotMatch(css, /\[data-style\s*=/, `Base sheet ${sheet} must not contain theme-specific [data-style=...] selectors`);
}

// 2. Each theme stylesheet may only target its own declared theme scope(s).
const THEME_SCOPES = {
  "nova.css": new Set(["nova"]),
  "nova-stage.css": new Set(["nova"]),
  "nova-shell.css": new Set(["nova"]),
  "nova-widgets.css": new Set(["nova"]),
};
for (const [sheet, allowed] of Object.entries(THEME_SCOPES)) {
  const css = stripComments(ui(sheet));
  const found = [...css.matchAll(/\[data-style="([^"]+)"\]/g)].map(m => m[1]);
  assert.ok(found.length > 0, `Theme sheet ${sheet} should scope rules with [data-style="..."]`);
  for (const style of found) {
    assert.ok(allowed.has(style), `Theme sheet ${sheet} leaked cross-theme selector [data-style="${style}"]`);
  }
}

// 3. Declarative Theme registry contract with Nova as default and Classic as fallback.
const ctx = {};
vm.runInNewContext(ui("theme.js"), ctx);
const Theme = ctx.Theme;
assert.equal(Theme.DEFAULTS.style, "nova", "Nova must be the default theme");
assert.deepEqual([...Theme.STYLES.map(s => s[0])], ["nova", "classic"], "Consolidated registry must expose Nova and Classic");
assert.equal(typeof Theme.get, "function", "Theme.get must be exported");
assert.equal(typeof Theme.has, "function", "Theme.has must be exported");
assert.equal(typeof Theme.register, "function", "Theme.register must be exported");
assert.equal(typeof Theme.active, "function", "Theme.active must be exported");

for (const [id, label] of Theme.STYLES) {
  const def = Theme.get(id);
  assert.equal(def.id, id, `Theme.get(${id}) must return matching id`);
  assert.equal(def.label, label, `Theme.get(${id}) must return matching label`);
  assert.ok(def.palettes?.light && def.palettes?.dark, `Theme ${id} must define light and dark palettes`);
  assert.ok(def.radii && def.radii["r-md"], `Theme ${id} must define radii tokens`);
  assert.ok(def.defaultAccent === "blue" || def.defaultAccent === "graphite", `Theme ${id} must define defaultAccent`);
  assert.equal(typeof def.features?.stage, "boolean", `Theme ${id} must declare boolean features.stage`);
}
assert.equal(Theme.has("topBar", "nova"), true, "Nova must declare topBar capability");
assert.equal(Theme.has("docksSidebar", "nova"), true, "Nova must declare docksSidebar capability");
assert.equal(Theme.has("pageCloseButton", "nova"), false, "Nova must declare pageCloseButton=false");
assert.equal(Theme.has("topBar", "classic"), false, "Classic must not declare topBar");
assert.equal(Theme.has("pageCloseButton", "classic"), true, "Classic must declare pageCloseButton=true");
assert.equal(Theme.get("prism").id, "classic", "Legacy prism alias resolves to classic");
assert.equal(Theme.get("aurora").id, "nova", "Legacy aurora alias resolves to nova");

// 4. Shared JS modules must query Theme capabilities instead of hardcoding `=== "nova"`.
const SHARED_JS = ["app.js", "aurora-rail.js", "aurora-avatar.js", "discover.js"];
for (const file of SHARED_JS) {
  const src = ui(file);
  assert.doesNotMatch(src, /dataset\.style\s*[!=]==?\s*"nova"/, `Shared module ${file} must use Theme.has()/Theme.get() instead of hardcoding dataset.style === "nova"`);
}

console.log("theme isolation & registry check ok");
