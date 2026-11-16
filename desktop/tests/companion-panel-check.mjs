// Companion inspector, file type filters, style-aware Appearance switching,
// modal ask/askText confirmations, and solid (unblurred) modal backdrops.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const uiDir = join(here, "../ui");
const read = f => readFileSync(join(uiDir, f), "utf8");
const app = read("app.js");
const themeSrc = read("theme.js");

function extractFn(src, signature) {
  const start = src.indexOf(signature);
  assert.ok(start >= 0, `missing ${signature}`);
  let i = src.indexOf("{", start), depth = 0, q = null;
  for (; i < src.length; i++) {
    const ch = src[i], prev = src[i - 1];
    if (q) { if (ch === q && prev !== "\\") q = null; continue; }
    if (ch === "'" || ch === '"' || ch === "`") { q = ch; continue; }
    if (ch === "{") depth++;
    else if (ch === "}") { depth--; if (depth === 0) return src.slice(start, i + 1); }
  }
  throw new Error(`unclosed ${signature}`);
}

const now = Date.now();
const recentIso = new Date(now - 3600_000).toISOString();
const oldIso = new Date(now - 14 * 86400_000).toISOString();
const sampleFiles = [
  { id: "f1", name: "brief.md", media_type: "text/markdown", size: 2048, source: "delivered", at: recentIso, origin: { kind: "upload" } },
  { id: "f2", name: "chart.png", media_type: "image/png", size: 4096, source: "delivered", at: recentIso, origin: { kind: "upload" } },
  { id: "f3", name: "report.pdf", media_type: "application/pdf", size: 8192, source: "uploaded", at: oldIso, origin: { kind: "upload" } },
];

const listeners = [];
const ctx = {
  esc: s => String(s ?? "").replace(/[&<>"]/g, c => `&#${c.charCodeAt(0)};`),
  enc: encodeURIComponent,
  when: at => `at ${at}`,
  kb: n => `${Math.max(1, Math.round(n / 1024))} KB`,
  icon: k => `<svg data-ic="${k}"></svg>`,
  paint: (el, html) => { el.innerHTML = html; },
  onLeave: () => {},
  $: (q, root) => (root && root._bySel && root._bySel(q)) || null,
  fileChip: f => `<a data-open="file:${f.id}">${f.name}</a>`,
  fileFrom: () => "Uploaded to Files",
  moveControl: () => "",
  trashButton: id => `<button data-trash="${id}">Delete</button>`,
  attachButton: label => `<button>${label}</button>`,
  plain: body => ({ body }),
  needsMe: () => [{ id: "n1" }, { id: "n2" }],
  ENDED: ["completed", "failed", "cancelled"],
  App: { me: { id: "alice" }, tasks: [{ status: "running" }], projects: [] },
  Panel: { lib: { q: "", project: "", kind: "all" } },
  Server: { files: { list: async () => ({ files: sampleFiles }) } },
  PANEL_TAB_PARENT: { activity: "activity", tasks: "activity", schedules: "activity", task: "activity", schedule: "activity", needs: "needs", rules: "needs", item: "needs", apps: "apps", connector: "apps", files: "files", file: "files" },
  localStorage: { getItem: () => null, setItem() {} },
  document: {
    documentElement: { style: { setProperty() {} }, dataset: {} },
    querySelectorAll: () => [],
    activeElement: null,
  },
  addEventListener: (t, fn) => listeners.push([t, fn]),
  dispatchEvent: e => { for (const [t, fn] of listeners) if (t === e.type) fn(e); },
  CustomEvent: class { constructor(type, o) { this.type = type; this.detail = o.detail; } },
  URL, Date, Math, Number, Set, Map, RegExp, Object, Array, String,
};
ctx.window = ctx;
ctx.globalThis = ctx;
vm.createContext(ctx);
vm.runInContext(themeSrc, ctx, { filename: "theme.js" });
const snippets = [
  extractFn(app, "function companionStatus()"),
  extractFn(app, "function companionHeader()"),
  extractFn(app, "function panelTabs(kind)"),
  extractFn(app, "function libFileKind(f)"),
  extractFn(app, "async function filesView()"),
  extractFn(app, "function appearanceSection(el)"),
].join("\n");
vm.runInContext(snippets, ctx, { filename: "app-slices.js" });

// 1. Companion header & tabs.
assert.match(ctx.companionHeader(), /<h2>Magnus<\/h2>/);
assert.match(ctx.companionHeader(), /2 waiting on you/);
assert.match(ctx.companionHeader(), /href="#\/settings\/appearance"[^>]*>.*Theme/);
ctx.needsMe = () => [];
assert.match(ctx.companionHeader(), /Working · 1 active/);
ctx.App.tasks = [];
assert.match(ctx.companionHeader(), /Ready/);
ctx.needsMe = () => [{ id: "n1" }, { id: "n2" }];
const tabs = ctx.panelTabs("rules");
for (const label of ["Activity", "Approvals", "Files", "Apps"]) assert.ok(tabs.includes(`<span>${label}</span>`), `missing tab ${label}`);
assert.match(tabs, /data-open="needs" aria-current="page".*<span class="badge">2<\/span>/);

// 2. File classification & Files filter pills + time grouping.
assert.equal(ctx.libFileKind(sampleFiles[0]), "pages");
assert.equal(ctx.libFileKind(sampleFiles[1]), "media");
assert.equal(ctx.libFileKind(sampleFiles[2]), "files");

const allRes = await ctx.filesView();
assert.match(allRes.body, /<div class="lib-group-head">This week<\/div>/);
assert.match(allRes.body, /<div class="lib-group-head">Older<\/div>/);
assert.match(allRes.body, /brief\.md/);
assert.match(allRes.body, /chart\.png/);
assert.match(allRes.body, /report\.pdf/);

ctx.Panel.lib.kind = "pages";
const pagesRes = await ctx.filesView();
assert.match(pagesRes.body, /brief\.md/);
assert.ok(!pagesRes.body.includes("chart.png") && !pagesRes.body.includes("report.pdf"));

ctx.Panel.lib.kind = "media";
const mediaRes = await ctx.filesView();
assert.match(mediaRes.body, /chart\.png/);
assert.ok(!mediaRes.body.includes("brief.md") && !mediaRes.body.includes("report.pdf"));

// 3. Appearance section: Theme selector, default auto-shift, and custom color preservation.
const makeEl = () => ({ innerHTML: "", isConnected: true, focus() {}, _bySel: () => null });
const appEl = makeEl();
ctx.appearanceSection(appEl);
assert.match(appEl.innerHTML, /<h3 id="style-h">Theme<\/h3>/);
assert.match(appEl.innerHTML, /data-look="style" data-value="antigravity"/);
assert.match(appEl.innerHTML, /data-look="style" data-value="companion"/);
assert.match(appEl.innerHTML, /data-custom-hex/);

const clickLook = (look, value) => appEl.onclick({ target: { closest: q => q === "[data-look]" ? { dataset: { look, value }, focus() {} } : null } });
assert.equal(ctx.Theme.prefs().style, "antigravity");
assert.equal(ctx.Theme.prefs().accent, "graphite");
clickLook("style", "companion");
assert.equal(ctx.Theme.prefs().style, "companion");
assert.equal(ctx.Theme.prefs().accent, "blue", "switching Antigravity -> Companion auto-shifts graphite -> blue");
clickLook("style", "antigravity");
assert.equal(ctx.Theme.prefs().style, "antigravity");
assert.equal(ctx.Theme.prefs().accent, "graphite", "switching Companion -> Antigravity auto-shifts blue -> graphite");
clickLook("accent", "emerald");
clickLook("style", "companion");
assert.equal(ctx.Theme.prefs().accent, "emerald", "explicit non-default accent stays when switching theme");

// 4. Rules panel handler and Panel.lib reset on stop().
assert.match(app, /\brules:\s*\(\)\s*=>\s*\(\{\s*body:\s*'<div class="rules-host"><\/div>',\s*after:\s*el\s*=>\s*rulesSection\(\$\("\.rules-host",\s*el\)\)\s*\}\)/);
assert.match(app, /Panel\.lib\s*=\s*\{\s*q:\s*"",\s*project:\s*"",\s*kind:\s*"all"\s*\}/);

// 5. No native browser confirm()/prompt() dialogs or backdrop-filter blur in desktop UI.
for (const file of ["app.js", "computers.js", "team-panel.js"]) {
  const src = read(file);
  assert.ok(!/(?:^|[^\w.])(?:window\.)?(?:confirm|prompt)\(/m.test(src), `${file} uses native confirm() or prompt()`);
}
for (const file of readdirSync(uiDir).filter(f => f.endsWith(".css"))) {
  assert.ok(!/backdrop-filter\s*:/i.test(read(file)), `${file} uses backdrop-filter`);
}

console.log("companion-panel-check: ok");
