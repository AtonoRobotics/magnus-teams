// The stylesheets stay easy to change (docs/ui-styles.md): each area has
// its own sheet, loaded in one order; a selector is written once; nothing is
// set that a later rule always overrides; every token exists; and no rule
// styles a class or id that nothing uses. Each of these is how an edit
// silently failed to show before (decisions/2026-11-16-one-place-per-style.md).
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const here = new URL(".", import.meta.url).pathname;
const ui = join(here, "../ui");
const root = join(here, "../..");
const read = p => readFileSync(p, "utf8");

/** The sheets, in the order the page loads them: later wins a tie. */
export const SHEETS = ["base.css", "layout.css", "chat.css", "pages.css", "teams.css", "computer.css", "admin.css", "onboarding.css", "aurora.css", "aurora-widgets.css", "aurora-stage.css", "aurora-rail.css", "aurora-compact.css", "nova.css"];
const SHEET_LINES_MAX = 800;

// 1. The page loads exactly these sheets in this order, and the server serves each.
const page = read(join(ui, "index.html"));
const linked = [...page.matchAll(/<link rel="stylesheet" href="([^"?]+)/g)].map(m => m[1]);
assert.deepEqual(linked, SHEETS, "index.html loads the stylesheets in the documented order");
const assets = read(join(root, "crates/magnus/src/ui/assets.rs"));
for (const s of SHEETS) assert.ok(assets.includes(`("${s}", "text/css`), `the server does not serve ${s}`);
assert.ok(!readdirSync(ui).includes("app.css"), "app.css is gone: its rules live in the area sheets");

// A small CSS reader: rules with their at-rule context, in load order.
function parse(text, ctx, sheet, out, base = 0) {
  let i = 0;
  while (i < text.length) {
    const open = nextBrace(text, i);
    if (open < 0) break;
    const prelude = text.slice(i, open).replace(/^[\s;]+/, "").trim();
    const close = matching(text, open);
    const body = text.slice(open + 1, close);
    if (prelude.startsWith("@")) {
      if (/^@(media|supports|container|layer)\b/.test(prelude)) parse(body, [...ctx, norm(prelude)], sheet, out, base + text.slice(0, open + 1).split("\n").length - 1);
    } else if (prelude) {
      out.push({ sheet, ctx: ctx.join(" | "), parts: splitList(prelude), decls: decls(body), line: base + text.slice(0, open).split("\n").length });
    }
    i = close + 1;
  }
  return out;
}
const norm = s => s.replace(/\s+/g, " ").trim();
function nextBrace(t, i) {
  for (let d = 0; i < t.length; i++) {
    const c = t[i];
    if (c === '"' || c === "'") { i = t.indexOf(c, i + 1); continue; }
    if (c === "(") d++; else if (c === ")") d--; else if (c === "{" && d === 0) return i;
  }
  return -1;
}
function matching(t, open) {
  for (let i = open, d = 0; i < t.length; i++) {
    const c = t[i];
    if (c === '"' || c === "'") { i = t.indexOf(c, i + 1); continue; }
    if (c === "{") d++; else if (c === "}" && --d === 0) return i;
  }
  throw new Error("unbalanced braces");
}
function splitList(sel) {
  const parts = []; let d = 0, cur = "";
  for (const c of sel) {
    if (c === "(" || c === "[") d++; else if (c === ")" || c === "]") d--;
    if (c === "," && d === 0) { parts.push(norm(cur)); cur = ""; } else cur += c;
  }
  return [...parts, norm(cur)].filter(Boolean);
}
function decls(body) {
  const out = []; let d = 0, cur = "";
  for (const c of body) {
    if (c === "(") d++; else if (c === ")") d--;
    if (c === ";" && d === 0) { push(cur); cur = ""; } else cur += c;
  }
  push(cur);
  return out;
  function push(s) { s = norm(s); const k = s.indexOf(":"); if (k > 0) out.push({ prop: s.slice(0, k).trim().toLowerCase(), value: s.slice(k + 1).trim(), text: s }); }
}

const rules = [];
const sources = {};
for (const s of SHEETS) {
  const text = read(join(ui, s));
  const lines = text.split("\n").length;
  assert.ok(lines <= SHEET_LINES_MAX, `${s} is ${lines} lines; split it by component (the limit is ${SHEET_LINES_MAX})`);
  sources[s] = text.replace(/\/\*[\s\S]*?\*\//g, m => m.replace(/[^\n]/g, " "));
  parse(sources[s], [], s, rules);
}

// 2. A property is set for a selector in one rule per context: a later rule
// setting it again (or a shorthand over it) means the earlier one never shows.
const SHORTHAND = {
  border: /^border-(top|right|bottom|left|block|inline)(-(start|end))?(-(width|style|color))?$|^border-(width|style|color)$/,
  "border-radius": /^border-(top|bottom|start|end)-(left|right|start|end)-radius$/,
  padding: /^padding-(top|right|bottom|left|block|inline)(-(start|end))?$/,
  margin: /^margin-(top|right|bottom|left|block|inline)(-(start|end))?$/,
  outline: /^outline-(color|style|width)$/,
  flex: /^flex-(grow|shrink|basis)$/,
  background: /^background-(color|image|position|size|repeat|origin|clip|attachment)$/,
  font: /^font-(family|size|weight|style|variant|stretch)$|^line-height$/,
  overflow: /^overflow-(x|y)$/, gap: /^(row|column)-gap$/, inset: /^(top|right|bottom|left)$/,
  transition: /^transition-/, animation: /^animation-/, "grid-template": /^grid-template-(rows|columns|areas)$/,
};
const covers = (later, earlier) => later === earlier || (SHORTHAND[later]?.test(earlier) ?? false);
const never = [];
const owner = new Map();
for (const r of rules) for (const p of r.parts) for (const d of r.decls) {
  if (/!important/.test(d.value)) continue;
  const prev = [...(owner.get(`${r.ctx} :: ${p}`) ?? [])].find(o => o.rule !== r && covers(d.prop, o.prop));
  if (prev) never.push(`${p}${r.ctx ? ` in ${r.ctx}` : ""} sets ${d.prop} at ${r.sheet}:${r.line} and ${prev.prop} at ${prev.rule.sheet}:${prev.rule.line}`);
  const k = `${r.ctx} :: ${p}`;
  if (!owner.has(k)) owner.set(k, []);
  owner.get(k).push({ rule: r, prop: d.prop });
}
for (const r of rules) r.decls.forEach((d, k) => {
  // within one rule a repeat is a fallback for older engines, which is fine
  // only when the values differ by a newer unit or function
  const again = r.decls.slice(k + 1).find(x => covers(x.prop, d.prop));
  if (again && !(again.prop === d.prop && /dvh|svh|lvh|-webkit-|color-mix|^clip$|var\(/.test(again.value) && again.value !== d.value))
    never.push(`${r.parts.join(", ")} at ${r.sheet}:${r.line} sets ${d.prop} twice`);
});
assert.deepEqual(never, [], `a property set for the same selector in two places (one never shows; keep the one that wins and change it there):\n  ${never.join("\n  ")}`);

// 3. Every token a sheet reads exists, unless it gives a fallback.
const theme = read(join(ui, "theme.js"));
const tokens = new Set([
  ...[...theme.matchAll(/(?:^|[{,]\s*)"?([a-z][a-z0-9-]*)"?\s*:/gm)].map(m => m[1]),
  ...Object.values(sources).flatMap(t => [...t.matchAll(/--([a-z0-9-]+)\s*:/g)].map(m => m[1])),
  ...readdirSync(ui).filter(f => f.endsWith(".js")).flatMap(f => [...read(join(ui, f)).matchAll(/setProperty\(\s*["'`]--([a-z0-9-]+)/g)].map(m => m[1])),
]);
const missing = [];
for (const [s, t] of Object.entries(sources))
  for (const m of t.matchAll(/var\(--([a-z0-9-]+)\s*(,)?/g)) if (!m[2] && !tokens.has(m[1])) missing.push(`${s}:${t.slice(0, m.index).split("\n").length} --${m[1]}`);
assert.deepEqual(missing, [], `tokens read but never defined (the declaration is dropped):\n  ${missing.join("\n  ")}`);

// 4. Every class and id a selector names is used by the page's code.
let code = readdirSync(ui).filter(f => /\.(js|html)$/.test(f)).map(f => read(join(ui, f))).join("\n");
(function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p); else if (/\.(rs|html)$/.test(f)) code += read(p);
  }
})(join(root, "crates/magnus/src"));
const used = w => new RegExp(`(?<![\\w-])${w.replace(/[-]/g, "\\-")}(?![\\w-])`).test(code);
const unused = new Set();
for (const r of rules) for (const p of r.parts)
  for (const m of p.replace(/\[[^\]]*\]|url\([^)]*\)/g, "").matchAll(/[.#]([A-Za-z_][\w-]*)/g)) if (!used(m[1])) unused.add(`${r.sheet}:${r.line} ${p} (${m[1]})`);
assert.deepEqual([...unused], [], `rules for classes or ids nothing uses (delete them):\n  ${[...unused].join("\n  ")}`);

console.log(`stylesheets ok: ${SHEETS.length} sheets, ${rules.length} rules`);
