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
const UI = {
  pencil: '<path/>', working: '<path/>', sparkles: '<path/>', bell: '<path/>',
  calendar: '<path/>', chev: '<path/>', globe: '<path/>', shield: '<path/>',
  done: '<path/>', need: '<path/>', stopped: '<path/>', grid: '<path/>',
  folder: '<path/>', team: '<path/>', trash: '<path/>', gear: '<path/>',
  palette: '<path/>', plus: '<path/>',
};
const ctx = {
  esc: s => String(s ?? "").replace(/[&<>"]/g, c => `&#${c.charCodeAt(0)};`),
  enc: encodeURIComponent,
  when: at => `at ${at}`,
  until: at => `in ${at}`,
  rule: () => "daily",
  firstLine: s => String(s || "").split("\n")[0],
  load: () => false,
  displayTitle: s => String(s || "Untitled"),
  guideNavItem: () => "",
  adminNavItem: () => "",
  isDev: () => false,
  STATE: t => t.status || "done",
  WAITING: { approval: "waiting approval" },
  KIND: { approval: "Approval" },
  card: i => `<article data-id="${i.id}">${i.kind}</article>`,
  askContent: () => "<div>details</div>",
  fieldValue: () => "<div>fields</div>",
  feedRows: () => "",
  needsDecision: i => i.status === "open" && i.can_respond,
  controlWords: (c) => ({ pill: c.status === "running" ? "Awake" : "Resting" }),
  standing: t => [t.status === "running" ? "working" : "done", `<span class="chip">${t.status}</span>`],
  kb: n => `${Math.max(1, Math.round(n / 1024))} KB`,
  UI,
  icon: (k, cls = "") => `<svg class="${cls}" data-ic="${k}"></svg>`,
  paint: (el, html) => { el.innerHTML = html; },
  onLeave: () => {},
  $: (q, root) => (root && root._bySel && root._bySel(q)) || null,
  fileChip: f => `<a data-open="file:${f.id}">${f.name}</a>`,
  fileFrom: () => "Uploaded to Files",
  moveControl: () => "",
  trashButton: id => `<button data-trash="${id}">Delete</button>`,
  attachButton: label => `<button>${label}</button>`,
  plain: body => ({ body }),
  needsMe: () => [{ id: "n1", thread: "t1" }, { id: "n2" }],
  ENDED: ["completed", "failed", "cancelled"],
  location: { hash: "#/chat/t1" },
  App: {
    me: { id: "alice", name: "Alice" },
    appliance: "local",
    thread: "t1",
    threads: [
      { thread: "t1", title: "Today chat", updated_at: recentIso },
      { thread: "t2", title: "Older chat", updated_at: oldIso },
    ],
    projects: [],
    workspaces: [],
    tasks: [{ task_id: "tk1", status: "running", thread: "t2", title: "Running job", created_at: recentIso }],
    schedules: [],
    inbox: [{ id: "n0", status: "decided", kind: "approval", summary: "Send quarterly email", resolution: { decision: "approve" }, created_at: oldIso }],
    computers: [{ id: "c1", name: "Magnus VM", status: "running" }],
  },
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
  extractFn(app, "function dayBucketLabel(iso)"),
  extractFn(app, "function activityLinks(selected=\"activity\")"),
  extractFn(app, "function companionHeader()"),
  extractFn(app, "function timelineGroups(active, done)"),
  extractFn(app, "function activityView(composed)"),
  extractFn(app, "function needsView(composed)"),
  extractFn(app, "function panelTabs(kind)"),
  extractFn(app, "function libFileKind(f)"),
  extractFn(app, "async function filesView()"),
  extractFn(app, "function navigationTree()"),
  extractFn(app, "function appearanceSection(el)"),
].join("\n");
vm.runInContext(snippets, ctx, { filename: "app-slices.js" });

// 1. Companion header & tabs.
const hdr = ctx.companionHeader();
assert.match(hdr, /<div class="companion-top-bar">/);
assert.match(hdr, /<div class="companion-avatar-wrap">/);
assert.match(hdr, /class="companion-edit-btn"/);
assert.match(hdr, /<h2>Magnus<\/h2>/);
assert.match(hdr, /2 waiting on you/);
assert.match(hdr, /Worker Companion/);
assert.match(hdr, /href="#\/settings\/appearance"[^>]*>.*Antigravity Theme/);
ctx.Theme.set({ style: "gemini" });
assert.match(ctx.companionHeader(), /Gemini Theme/);
ctx.Theme.set({ style: "antigravity" });
ctx.needsMe = () => [];
assert.match(ctx.companionHeader(), /Working · 1 active/);
ctx.App.tasks = [];
assert.match(ctx.companionHeader(), /Ready/);
ctx.needsMe = () => [{ id: "n1", thread: "t1" }, { id: "n2" }];
const tabs = ctx.panelTabs("rules");
for (const label of ["Activity", "Approvals", "Files", "Apps"]) assert.ok(tabs.includes(`<span>${label}</span>`), `missing tab ${label}`);
assert.match(tabs, /data-open="needs" aria-current="page".*<span class="badge">2<\/span>/);

// 2. Grobot Activity, Timeline, Approvals History, and Sidebar tree.
const act = ctx.activityView([]);
assert.match(act, /<div class="gbox">.*Create a routine.*Discover automations.*Enable Heartbeat/s);
assert.match(act, /class="comp-mini-bar"/);
assert.match(act, /<details class="companion-scheds" data-key="panel-schedules" open>/);
assert.match(act, /<a class="gbox-footer" href="#\/schedules"/);
assert.match(act, /<nav class="activity-views" aria-label="Activity views">/);

const tl = ctx.timelineGroups([{ task_id: "tk1", status: "running", title: "Running job", created_at: recentIso }], []);
assert.match(tl, /<details class="tl-day" data-key="day:Active now" open>/);
assert.match(tl, /<ul class="tlist tl-stack">/);

const nv = ctx.needsView([]);
assert.match(nv, /class="gbox companion-guard"/);
assert.match(nv, /class="appr-history".*class="appr-ic mail".*class="gbox appr-payload-box"/s);

ctx.App.tasks = [{ task_id: "tk1", status: "running", thread: "t2", title: "Running job", created_at: recentIso }];
const tree = ctx.navigationTree();
assert.match(tree, /<div class="group"><span>Today<\/span><\/div>/);
assert.match(tree, /<div class="group"><span>Previous<\/span><\/div>/);
assert.match(tree, /class="nav-dot warn"/);
assert.match(tree, /class="nav-dot run"/);

// 3. File classification & Files filter pills + time grouping + type badges.
assert.equal(ctx.libFileKind(sampleFiles[0]), "pages");
assert.equal(ctx.libFileKind(sampleFiles[1]), "media");
assert.equal(ctx.libFileKind(sampleFiles[2]), "files");

const allRes = await ctx.filesView();
assert.match(allRes.body, /<div class="lib-group-head">This week<\/div>/);
assert.match(allRes.body, /<div class="lib-group-head">Older<\/div>/);
assert.match(allRes.body, /<ul class="flist gbox">/);
assert.match(allRes.body, /class="lib-badge page">MD/);
assert.match(allRes.body, /class="lib-badge media">IMG/);
assert.match(allRes.body, /class="lib-badge pdf">PDF/);

ctx.Panel.lib.kind = "pages";
const pagesRes = await ctx.filesView();
assert.match(pagesRes.body, /brief\.md/);
assert.ok(!pagesRes.body.includes("chart.png") && !pagesRes.body.includes("report.pdf"));

ctx.Panel.lib.kind = "media";
const mediaRes = await ctx.filesView();
assert.match(mediaRes.body, /chart\.png/);
assert.ok(!mediaRes.body.includes("brief.md") && !mediaRes.body.includes("report.pdf"));

// 4. Appearance section: Theme selector, default auto-shift, and custom color preservation.
const makeEl = () => ({ innerHTML: "", isConnected: true, focus() {}, _bySel: () => null });
const appEl = makeEl();
ctx.appearanceSection(appEl);
assert.match(appEl.innerHTML, /<h3 id="style-h">Theme<\/h3>/);
assert.match(appEl.innerHTML, /data-look="style" data-value="antigravity"/);
assert.match(appEl.innerHTML, /data-look="style" data-value="gemini"/);
assert.match(appEl.innerHTML, /data-custom-hex/);

const clickLook = (look, value) => appEl.onclick({ target: { closest: q => q === "[data-look]" ? { dataset: { look, value }, focus() {} } : null } });
assert.equal(ctx.Theme.prefs().style, "antigravity");
assert.equal(ctx.Theme.prefs().accent, "graphite");
clickLook("style", "gemini");
assert.equal(ctx.Theme.prefs().style, "gemini");
assert.equal(ctx.Theme.prefs().accent, "blue", "switching Antigravity -> Gemini auto-shifts graphite -> blue");
clickLook("style", "antigravity");
assert.equal(ctx.Theme.prefs().style, "antigravity");
assert.equal(ctx.Theme.prefs().accent, "graphite", "switching Gemini -> Antigravity auto-shifts blue -> graphite");
clickLook("accent", "emerald");
clickLook("style", "gemini");
assert.equal(ctx.Theme.prefs().accent, "emerald", "explicit non-default accent stays when switching theme");

// 5. Desktop auto-open, rules panel handler, and dual-theme CSS rules.
assert.match(app, /if\s*\(!targetPanel\s*&&\s*!Panel\.what\s*&&\s*!Panel\.userClosed\s*&&\s*typeof innerWidth === "number"\s*&&\s*innerWidth >= 1200\)/);
assert.match(app, /if\s*\(work\s*&&\s*Panel\.autoOpened\s*&&\s*\(shell\.clientWidth < 1200 \|\| !m\.eligible\)\)/);
assert.match(app, /\brules:\s*\(\)\s*=>\s*\(\{\s*body:\s*'<div class="rules-host"><\/div>',\s*after:\s*el\s*=>\s*rulesSection\(\$\("\.rules-host",\s*el\)\)\s*\}\)/);
assert.match(app, /Panel\.lib\s*=\s*\{\s*q:\s*"",\s*project:\s*"",\s*kind:\s*"all"\s*\}/);

assert.match(read("chat.css"), /html\[data-style="gemini"\]\s*\.msg\.me/);
assert.match(read("layout.css"), /html\[data-style="gemini"\]\s*\.panel-tabs/);
assert.match(read("pages.css"), /html\[data-style="gemini"\]\s*\.gbox/);

// 6. No native browser confirm()/prompt() dialogs or backdrop-filter blur in desktop UI.
for (const file of ["app.js", "computers.js", "team-panel.js"]) {
  const src = read(file);
  assert.ok(!/(?:^|[^\w.])(?:window\.)?(?:confirm|prompt)\(/m.test(src), `${file} uses native confirm() or prompt()`);
}
for (const file of readdirSync(uiDir).filter(f => f.endsWith(".css"))) {
  assert.ok(!/backdrop-filter\s*:/i.test(read(file)), `${file} uses backdrop-filter`);
}

console.log("companion-panel-check: ok");
