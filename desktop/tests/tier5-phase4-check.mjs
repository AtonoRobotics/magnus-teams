// Regression tests for Tier 5 Phase 4:
//   - aurora-reasoning.js and aurora.js retired their fallback MutationObservers
//   - app-library.js and app-settings.js extracted cleanly (<800L each) and
//     registered in index.html + crates/magnus/src/ui/assets.rs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");
const lines = rel => read(rel).split("\n").length;

// 1. Final Aurora fallback MutationObservers retired
const reasoning = read("desktop/ui/aurora-reasoning.js");
assert.doesNotMatch(reasoning, /new MutationObserver/, "aurora-reasoning.js must not instantiate a MutationObserver");
assert.match(reasoning, /magnus-render/, "aurora-reasoning.js listens for magnus-render");

const aurora = read("desktop/ui/aurora.js");
assert.doesNotMatch(aurora, /new MutationObserver\(handleDom\)/, "aurora.js must not instantiate fallback handleDom MutationObserver");
assert.match(aurora, /magnus-render/, "aurora.js listens for magnus-render");

// 2. Extracted modules and app.js are all strictly < 800 lines and registered
for (const m of ["app.js", "app-shell.js", "app-files.js", "app-polling.js", "app-chat.js", "app-inbox.js", "app-panel.js", "app-library.js", "app-settings.js", "app-boot.js"]) {
  assert.ok(lines(`desktop/ui/${m}`) < 800, `${m} must be < 800 lines (got ${lines(`desktop/ui/${m}`)})`);
}

const indexHtml = read("desktop/ui/index.html");
assert.match(indexHtml, /<script src="app-shell\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-chat\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-inbox\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-panel\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-library\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-settings\.js"><\/script>/);
assert.match(indexHtml, /<script src="app-boot\.js"><\/script>/);
const assetsRs = read("crates/magnus/src/ui/assets.rs");
for (const m of ["app-shell.js", "app-chat.js", "app-inbox.js", "app-panel.js", "app-library.js", "app-settings.js", "app-boot.js"]) {
  assert.match(assetsRs, new RegExp(m.replace(".", "\\.")));
}

import vm from "node:vm";

// 3. Sequential browser <script> load-order check across all app*.js modules
{
  const appScripts = [...indexHtml.matchAll(/<script src="([^"]+)"><\/script>/g)]
    .map(m => m[1])
    .filter(s => s === "app.js" || s.startsWith("app-"));
  const noop = () => {};
  const mkEl = () => ({ style: { setProperty: noop }, dataset: {}, classList: { add: noop, remove: noop, toggle: noop, contains: () => false }, addEventListener: noop, removeEventListener: noop, querySelector: () => null, querySelectorAll: () => [], getAttribute: () => null, setAttribute: noop, append: noop, remove: noop });
  const doc = { readyState: "complete", hidden: false, documentElement: mkEl(), body: mkEl(), head: mkEl(), activeElement: null, querySelector: () => null, querySelectorAll: () => [], getElementById: () => null, createElement: () => mkEl(), addEventListener: noop, removeEventListener: noop, dispatchEvent: noop };
  const ctx = vm.createContext({
    window: null, document: doc, navigator: { clipboard: {} },
    location: { hash: "#/home", origin: "http://localhost", pathname: "/", protocol: "http:", host: "localhost", replace: noop, reload: noop },
    localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
    sessionStorage: { getItem: () => null, setItem: noop, removeItem: noop },
    addEventListener: noop, removeEventListener: noop, dispatchEvent: noop,
    setTimeout: () => 1, clearTimeout: noop, setInterval: () => 1, clearInterval: noop,
    requestAnimationFrame: () => 1, cancelAnimationFrame: noop, queueMicrotask: noop,
    CustomEvent: class { constructor(t, o) { this.type = t; this.detail = o?.detail; } },
    Event: class { constructor(t) { this.type = t; } },
    URLSearchParams, Date, Map, Set, WeakMap, WeakSet, Promise, JSON, Math, RegExp, Object, Array, Number, String, Boolean, Error,
    MutationObserver: class { observe() {} disconnect() {} },
    ResizeObserver: class { observe() {} disconnect() {} },
    CSS: { escape: s => String(s) },
    Magnus: { kind: "web", restore: async () => null, updateState: async () => null, onUpdateReady: noop, systemLook: async () => ({}) },
    Server: { chat: {}, inbox: {}, tasks: {}, schedules: {}, projects: {}, connectors: {}, workspaces: {}, computers: {}, models: {}, preferences: {}, version: async () => ({}), staged: {}, briefing: async () => null, homeOpened: async () => null, spend: {}, memory: {}, rules: {}, outsideAgents: {}, local: {}, admin: {}, stops: {} },
    Theme: { STYLES: [["nova", "Nova"], ["classic", "Classic"]], DEFAULTS: { style: "nova" }, active: () => "nova", get: () => ({ id: "nova", features: {} }), has: () => false, stage: () => true, prefs: () => ({ style: "nova" }), os: () => ({}), effective: () => ({ style: "nova", mode: "dark" }), scope: noop, set: noop, LAYOUT_BOUNDS: { autoOpenMinWidthPx: 1200, panelMinWidthPx: 300, panelMaxWidthPx: 560, panelDefaultWidthPx: 360 } },
    Notices: { show: noop, poll: async () => {}, syncInbox: () => false, setSystem: noop, systemOn: () => false },
    UIStore: { get: () => ({}), patch: noop, setTask: noop, setTasks: noop, subscribe: () => noop, hasState: () => false, derivePhase: () => "ready" },
    AuroraCore: { onDom: noop, onState: noop, onMain: noop, onFrame: noop },
    AuroraBoxes: { scope: noop },
    AuroraStage: { refresh: async () => {} },
    NovaPages: { showBriefing: noop },
    MobileApprove: { show: noop },
    Projects: { showList: noop, show: noop },
    Discover: { show: noop },
    PlanSurface: { renderLiveCard: () => "" },
    ExploreIdeas: { open: () => [], isNotice: () => false, isIdea: () => false },
    ConnectWeb: { finish: async () => {} },
    AdminState: { standing: null },
    console,
  });
  ctx.window = ctx;
  ctx.globalThis = ctx;
  for (const s of appScripts) {
    vm.runInContext(read(`desktop/ui/${s}`), ctx, { filename: s });
  }
}

console.log(`tier5-phase4-check: ok (app.js=${lines("desktop/ui/app.js")}L, app-library.js=${lines("desktop/ui/app-library.js")}L, app-settings.js=${lines("desktop/ui/app-settings.js")}L)`);
