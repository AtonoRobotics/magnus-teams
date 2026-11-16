// Theme variable injection, live-update script, and height clamping for magnus-html frames.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert";
const here = new URL(".", import.meta.url).pathname;
const ctx = {};
vm.runInNewContext(readFileSync(new URL("../ui/theme.js", import.meta.url), "utf8"), ctx);
const Theme = ctx.Theme;
const HtmlFrame = createRequire(import.meta.url)(`${here}../ui/html-frame.js`);

const darkLook = Theme.effective({ mode: "dark", accent: "blue" }, { dark: true });
const lightLook = Theme.effective({ mode: "light", accent: "emerald" }, { dark: false });

const darkVars = HtmlFrame.themeVars(Theme.tokens(darkLook), darkLook.mode);
assert.strictEqual(darkVars["--foreground"], "#ECEFF4");
assert.strictEqual(darkVars["--primary"], "#7C8CFF");
assert.ok(darkVars["--chart-1"] && darkVars["--chart-6"]);

const lightVars = HtmlFrame.themeVars(Theme.tokens(lightLook), lightLook.mode);
assert.strictEqual(lightVars["--foreground"], "#111318");
assert.strictEqual(lightVars["--primary"], "#146C34");

// Fragment wrapping injects style, script, transparent body, and ResizeObserver.
const frag = HtmlFrame.wrap("<div>Hello</div>", Theme.tokens(darkLook), "dark");
assert.ok(frag.startsWith('<!doctype html><html class="dark" style="color-scheme:dark">'));
assert.ok(frag.includes('<style id="magnus-html-theme">:root{'));
assert.ok(frag.includes("background:transparent"));
assert.ok(frag.includes("magnus:resize"));
assert.ok(frag.includes("magnus:theme"));
assert.ok(frag.includes("<body><div>Hello</div></body>"));

// Full document with <head> keeps outer doctype and injects inside <head>.
const full = HtmlFrame.wrap("<!doctype html><html><head><title>X</title></head><body><p>Y</p></body></html>", Theme.tokens(lightLook), "light");
assert.strictEqual(full.match(/<!doctype html>/gi).length, 1);
assert.ok(full.includes('<head><style id="magnus-html-theme">'));
assert.ok(full.includes("<title>X</title>"));

// Height clamping respects MIN_FRAME_HEIGHT_PX and MAX_INLINE_HEIGHT_PX.
assert.strictEqual(HtmlFrame.clampHeight(10), HtmlFrame.MIN_FRAME_HEIGHT_PX);
assert.strictEqual(HtmlFrame.clampHeight(240.4), 241);
assert.strictEqual(HtmlFrame.clampHeight(2000), HtmlFrame.MAX_INLINE_HEIGHT_PX);
assert.strictEqual(HtmlFrame.clampHeight(NaN), null);

// Malicious token values cannot break out of <style>.
const sanitized = HtmlFrame.themeVars({ fg: "red</style><script>alert(1)</script>" }, "light");
assert.ok(!sanitized["--fg"].includes("</style"));
assert.ok(!sanitized["--fg"].includes(";"));

console.log("html frame ok");
