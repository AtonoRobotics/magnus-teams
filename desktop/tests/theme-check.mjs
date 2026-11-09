// Every look's text is readable (U7, amended): normal text reaches 4.5:1
// (WCAG 2.2, 1.4.3) on every surface it sits on. Glass surfaces are checked
// over the worst-case backdrop: whatever is behind the window, from black to
// white, and the drawn wallpaper's colors; blurring the backdrop only mixes
// its colors, so its extremes bound every case. Reading surfaces are checked
// as they are, solid in every look.
//
// node desktop/tests/theme-check.mjs     prints the lowest ratio of each look
import { readFileSync } from "node:fs";
import vm from "node:vm";

const ctx = {};
vm.runInNewContext(readFileSync(new URL("../ui/theme.js", import.meta.url), "utf8"), ctx);
const Theme = ctx.Theme;
const MIN = 4.5;

const rgba = c => {
  c = c.trim();
  let m = c.match(/^#([0-9a-f]{6})$/i);
  if (m) return [0, 2, 4].map(i => parseInt(m[1].slice(i, i + 2), 16)).concat(1);
  m = c.match(/^rgba?\(([^)]+)\)$/);
  if (!m) throw new Error(`not a color: ${c}`);
  const p = m[1].split(",").map(Number);
  return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
};
// `top` laid over the opaque `under`.
const over = (top, under) => [0, 1, 2].map(i => top[i] * top[3] + under[i] * (1 - top[3])).concat(1);
const lum = c => {
  const [r, g, b] = c.slice(0, 3).map(v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

// Text as each look draws it, by the surface under it. A surface is a stack
// of tokens, bottom first; text on a highlight (a selected row, the search
// field, a segmented control) is checked on that highlight too.
// The pairs the Magnus Workspace design draws (owner, 2026-11-01: match the
// references exactly), each text on the surfaces it sits on there: on the
// chrome, the sidebar's labels and counts, a selected row's title, the
// search field's placeholder, the segmented control's labels; on reading
// surfaces, the conversation, cards and tiles.
const TEXT = ["fg", "fg2", "muted", "accent-text"];
const CHROME_PAIRS = s => [
  // Accent-colored text sits on cards, never on the glass itself.
  ...["fg", "fg2", "muted"].map(fg => [fg, [s]]),
  ["fg", [s, "row-sel"]], ["fg", [s, "search"]], ["search-muted", [s, "search"]],
  ["fg2", [s, "seg"]], ["fg", [s, "seg", "seg-on"]],
];
const READING = [["main"], ["surface"], ["raised"], ["group-head"]];
const READING_PAIRS = [...READING.flatMap(l => TEXT.map(fg => [fg, l])), ["fg", ["bubble"]], ["fg", ["approval-bg"]], ["fg2", ["approval-bg"]], ["fg", ["seg-on"]], ["fg2", ["seg"]]];
const PAIRS = [["accent-fg", "accent"], ["ok", "ok-soft"], ["warn", "warn-soft"], ["sch", "sch-soft"], ["bad", "bad-soft"], ["warn", "approval-bg"], ["accent-text", "accent-soft"]];

// The backdrops that can be behind glass: the system's material over
// anything on the desktop (its tint over black, white and grey between, the
// luminance extremes; Windows 11's Mica stays within them), and the drawn
// wallpaper's colors where the system has no material.
const wallpaper = look => [...Theme.GLASS[look.mode].backdrop.matchAll(/#[0-9a-f]{6}/gi)].map(m => rgba(m[0]));
const backdrops = look => ["#000000", "#ffffff", "#808080"].map(d => over(rgba(Theme.GLASS[look.mode].system), rgba(d))).concat(wallpaper(look));

const failures = [];
const report = [];
for (const mode of ["light", "dark"]) for (const material of ["solid", "glass"]) for (const accent of Theme.ACCENTS.map(a => a.id)) {
  const look = { mode, material, accent, motion: "full" };
  const t = Theme.tokens(look);
  const name = `${mode}/${material}/${accent}`;
  let low = Infinity;
  const check = (fg, layers, what) => {
    const unders = Theme.GLASS_SURFACES.includes(layers[0]) && material === "glass" ? backdrops(look) : [rgba(t.bg)];
    for (const under of unders) {
      const bgc = layers.reduce((acc, k) => over(rgba(t[k]), acc), under);
      const r = ratio(rgba(t[fg]), bgc);
      low = Math.min(low, r);
      if (r < MIN) failures.push(`${name}: ${fg} on ${layers.join(" over ")}${unders.length > 1 ? ` over ${under.slice(0, 3).join(",")}` : ""} is ${r.toFixed(2)}:1 (${what})`);
    }
  };
  for (const s of ["side", "panel"]) for (const [fg, layers] of CHROME_PAIRS(s)) check(fg, layers, "chrome");
  for (const [fg, layers] of READING_PAIRS) check(fg, layers, "reading");
  for (const [fg, bg] of PAIRS) check(fg, [bg], "label");
  // Reading surfaces stay solid in every look.
  for (const [k] of [...READING, ["bubble"], ["approval-bg"], ["seg-on"]]) if (rgba(t[k])[3] !== 1) failures.push(`${name}: the reading surface ${k} is not solid (${t[k]})`);
  report.push(`${name.padEnd(24)} lowest ${low.toFixed(2)}:1`);
}

// Glass only where the decision record allows it, and never when the
// system asks for less transparency or the person chose solid.
const os = { dark: false, reduceMotion: false, reduceTransparency: true };
if (Theme.effective({ material: "glass" }, os).material !== "solid") failures.push("reduced transparency did not fall back to solid");
if (Theme.effective({ motion: "full" }, { ...os, reduceMotion: true }).motion !== "reduced") failures.push("the system's reduced motion did not reduce motion");
if (Theme.effective({ motion: "off" }, { ...os, reduceMotion: true }).motion !== "off") failures.push("the system's reduced motion turned motion back on");
if (Theme.effective({ mode: "system" }, { ...os, dark: true }).mode !== "dark") failures.push("Match computer did not follow the system into dark");

console.log(report.join("\n"));
if (failures.length) { console.error(`\n${failures.length} below ${MIN}:1 or wrong:\n${failures.join("\n")}`); process.exit(1); }
console.log(`every look reaches ${MIN}:1`);
await import('./preferences-check.mjs');

await import("./native-material-check.mjs");
