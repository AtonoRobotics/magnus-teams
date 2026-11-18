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
vm.runInContext(read("evidence.js"), ctx, { filename: "evidence.js" });
vm.runInContext(`${read("app-logos.js")}\nglobalThis.AppLogos = AppLogos;`, ctx, { filename: "app-logos.js" });
const snippets = [
  extractFn(app, "function companionStatus()"),
  extractFn(app, "function dayBucketLabel(iso)"),
  extractFn(app, "function activityLinks(selected=\"activity\")"),
  extractFn(app, "function companionHeader()"),
  extractFn(app, "function companionFooter()"),
  extractFn(app, "function timelineRowHtml(t)"),
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

// 1. Companion header & tabs across all 4 current themes.
const hdr = ctx.companionHeader();
assert.match(hdr, /<div class="companion-row-left">/);
assert.match(hdr, /class="companion-avatar"/);
assert.match(hdr, /<span class="companion-m">M<\/span>/);
assert.match(hdr, /<h2>Magnus<\/h2>/);
assert.match(hdr, /Working/);
assert.match(hdr, /<span class="muted">1 task today<\/span>/, "when connectors are not yet loaded, do not show a hardcoded app count");
ctx.App.connectors = [];
assert.match(ctx.companionHeader(), /1 task today · 0 apps/, "shows 0 apps when /v1/connectors returns an empty or disconnected list");
ctx.App.connectors = [
  { name: "github", state: "connected" },
  { name: "gmail", state: "connected" },
  { name: "linear", state: "available" },
];
assert.match(ctx.companionHeader(), /1 task today · 2 apps/, "shows real connected app count when connectors are loaded");
assert.match(app, /get\("\/v1\/connectors",\s*"connectors"\)/, "refresh() fetches /v1/connectors from the server");
ctx.needsMe = () => [];
assert.match(ctx.companionHeader(), /Working/);
ctx.App.tasks = [];
assert.match(ctx.companionHeader(), /Ready/);
ctx.App.tasks = [{ task_id: "tk1", status: "running", thread: "t2", title: "Review Linear issue #84", created_at: recentIso, status_line: "Reading issue #84" }];
ctx.needsMe = () => [{ id: "n1", thread: "t1", summary: "Send Gmail update" }, { id: "n2", summary: "Create GitHub PR" }];
const tabs = ctx.panelTabs("rules");
for (const label of ["Activity", "Approvals", "Files", "Apps"]) assert.ok(tabs.includes(`<span>${label}</span>`), `missing tab ${label}`);
assert.match(tabs, /data-open="needs" aria-current="page".*<span class="badge">2<\/span>/);

// 2. Activity priority ordering, inline approval banner, Active now progress card, Timeline brand logos, Approvals History brand logos, and Sidebar tree.
const act = ctx.activityView([]);
assert.match(act, /class="companion-sec companion-needs-banner"/);
assert.match(act, /Needs your approval \(2\)/);
assert.match(act, /class="gbox needs-inline-card"/);
assert.match(act, /class="gbox companion-active-card"/);
assert.doesNotMatch(act, /class="active-progress/, "no fake progress bar when task has no plan units");
ctx.needsMe = () => [];
ctx.App.tasks = [{
  task_id: "tk1",
  status: "running",
  thread: "t2",
  title: "Review Linear issue #84",
  created_at: recentIso,
  status_line: "Reading issue #84",
  plan: { units: [{ id: "u1", status: "done" }, { id: "u2", status: "done" }, { id: "u3", status: "active" }, { id: "u4", status: "pending" }] },
}];
const actWithPlan = ctx.activityView([]);
assert.match(actWithPlan, /class="active-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="50"><span style="width:50%"><\/span><\/div>/);
assert.match(actWithPlan, /data-status-line="tk1"/, "running task foot uses real statusLineSlot from evidence.js");
const slotNode = { dataset: { statusLine: "tk1" }, textContent: "Working on it" };
ctx.liveStatusLines({ querySelectorAll: () => [slotNode] });
assert.equal(slotNode.textContent, "Reading issue #84");
ctx.needsMe = () => [{ id: "n1", thread: "t1", summary: "Send Gmail update" }, { id: "n2", summary: "Create GitHub PR" }];
assert.match(act, /<section class="companion-scheds">/);
assert.match(act, /<a class="sec-head-link" href="#\/schedules"/);
assert.match(act, /<nav class="activity-views" aria-label="Activity views">/);
assert.ok(act.indexOf("companion-needs-banner") < act.indexOf("companion-active-card"), "Inline approval banner appears above Active now card");
assert.ok(act.indexOf("companion-active-card") < act.indexOf("companion-timeline"), "Active now card appears above Recent work timeline");
assert.ok(act.indexOf("companion-timeline") < act.indexOf("companion-scheds"), "Recent work appears above Up next schedules");
const ftr = ctx.companionFooter();
assert.match(ftr, /class="companion-computer"/);
assert.match(ftr, /Cloud Computer ·/);
assert.match(ftr, /data-open="computer:c1">Watch →<\/button>/);
ctx.App.computers = [{ id: "c1", name: "Magnus VM", status: "stopped" }];
assert.match(ctx.companionFooter(), /data-wake-computer="c1">Wake VM →<\/button>/);
ctx.App.computers = [];
assert.equal(ctx.companionFooter(), "", "no fake computer bar when no computer is provisioned");
ctx.App.computers = [{ id: "c1", name: "Magnus VM", status: "running" }];

const tl = ctx.timelineGroups([{ task_id: "tk1", status: "running", title: "Review GitHub PR #42", created_at: recentIso }], []);
assert.match(tl, /<details class="tl-day" data-key="day:Active now" open>/);
assert.match(tl, /<ul class="tlist tl-stack">/);
assert.match(tl, /<span class="tl-brand" aria-hidden="true"><svg class="ctile-svg"/);

const nv = ctx.needsView([]);
assert.match(nv, /companion-sec-head"><span>Approvals<\/span>/);
assert.doesNotMatch(nv, /brand SVGs/);
assert.match(nv, /class="gbox companion-guard"/);
assert.match(nv, /class="appr-history".*class="appr-ic mail has-logo".*<svg class="ctile-svg".*class="gbox appr-payload-box"/s);

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
assert.match(allRes.body, /class="lib-badge page"><svg[^>]*>.*<span class="sr">MD<\/span>/);
assert.match(allRes.body, /class="lib-badge media"><svg[^>]*>.*<span class="sr">IMG<\/span>/);
assert.match(allRes.body, /class="lib-badge pdf"><svg[^>]*>.*<span class="sr">PDF<\/span>/);

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
assert.match(appEl.innerHTML, /data-look="style" data-value="obsidian"/);
assert.match(appEl.innerHTML, /data-look="style" data-value="obsidian-2"/);
assert.match(appEl.innerHTML, /data-look="style" data-value="prism"/);
assert.match(appEl.innerHTML, /data-look="style" data-value="studio"/);
assert.match(appEl.innerHTML, /data-custom-hex/);

const clickLook = (look, value) => appEl.onclick({ target: { closest: q => q === "[data-look]" ? { dataset: { look, value }, focus() {} } : null } });
assert.equal(ctx.Theme.prefs().style, "obsidian");
assert.equal(ctx.Theme.prefs().accent, "graphite");
clickLook("style", "prism");
assert.equal(ctx.Theme.prefs().style, "prism");
assert.equal(ctx.Theme.prefs().accent, "blue", "switching Obsidian -> Prism auto-shifts graphite -> blue");
clickLook("style", "obsidian");
assert.equal(ctx.Theme.prefs().style, "obsidian");
assert.equal(ctx.Theme.prefs().accent, "graphite", "switching Prism -> Obsidian auto-shifts blue -> graphite");
clickLook("style", "obsidian-2");
assert.equal(ctx.Theme.prefs().style, "obsidian-2");
assert.equal(ctx.Theme.prefs().accent, "blue", "switching Obsidian -> Obsidian 2 auto-shifts graphite -> blue");
clickLook("style", "studio");
assert.equal(ctx.Theme.prefs().style, "studio");
clickLook("accent", "emerald");
clickLook("style", "prism");
assert.equal(ctx.Theme.prefs().accent, "emerald", "explicit non-default accent stays when switching theme");

// 5. Desktop auto-open, rules panel handler, and multi-theme CSS rules.
assert.match(app, /if\s*\(!targetPanel\s*&&\s*!Panel\.what\s*&&\s*!Panel\.userClosed\s*&&\s*typeof innerWidth === "number"\s*&&\s*innerWidth >= Theme\.LAYOUT_BOUNDS\.autoOpenMinWidthPx\)/);
assert.match(app, /if\s*\(work\s*&&\s*Panel\.autoOpened\s*&&\s*\(shell\.clientWidth < B\.autoOpenMinWidthPx \|\| !m\.eligible\)\)/);
assert.match(app, /\brules:\s*\(\)\s*=>\s*\(\{\s*body:\s*'<div class="rules-host"><\/div>',\s*after:\s*el\s*=>\s*rulesSection\(\$\("\.rules-host",\s*el\)\)\s*\}\)/);
assert.match(app, /Panel\.lib\s*=\s*\{\s*q:\s*"",\s*project:\s*"",\s*kind:\s*"all"\s*\}/);

assert.match(read("chat.css"), /html\[data-style="prism"\]\s*\.msg\.me/);
assert.match(read("chat.css"), /html\[data-style="obsidian-2"\]\s*\.fchip-inline/);
assert.match(read("chat.css"), /html\[data-style="studio"\]\s*\.hello-hero/);
assert.match(read("layout.css"), /html\[data-style="prism"\]\s*\.panel-tabs/);
assert.match(read("layout.css"), /html\[data-style="obsidian-2"\]\s*\.shell/);
assert.match(read("layout.css"), /html\[data-style="studio"\]\s*nav\.side/);
assert.match(read("pages.css"), /\.gbox\s*\{\s*border:\s*0;/);

// 6. No native browser confirm()/prompt() dialogs or backdrop-filter blur in desktop UI.
for (const file of ["app.js", "computers.js", "team-panel.js"]) {
  const src = read(file);
  assert.ok(!/(?:^|[^\w.])(?:window\.)?(?:confirm|prompt)\(/m.test(src), `${file} uses native confirm() or prompt()`);
}
for (const file of readdirSync(uiDir).filter(f => f.endsWith(".css"))) {
  assert.ok(!/backdrop-filter\s*:/i.test(read(file)), `${file} uses backdrop-filter`);
}

assert.match(hdr, /data-admin-kill/);
assert.ok(!app.includes("inspector-pill"), "Companion pill removed from conversation top bar");
assert.match(read("chat-composer.js"), /send\.classList\.toggle\("is-stop",\s*stopMode\)/);

console.log("companion-panel-check: ok");
