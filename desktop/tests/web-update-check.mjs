// A browser tab follows the server's updates
// (decisions/2026-11-21-web-tab-follows-updates.md), with web-update.js as
// it is, in a stand-in page:
// - a tab served as one version, when the server now runs another, loads
//   the new page while nothing is being done in it;
// - not while the person is typing in it, or a dialog is open, or just
//   after they clicked: then as soon as they leave the tab;
// - once per version, so a page still stamped old after loading does not
//   load over and over; nothing when the version is the same, or the server
//   cannot be reached;
// - the desktop app is left alone.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const code = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../ui/web-update.js"), "utf8");
const index = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../ui/index.html"), "utf8");
assert.ok(/<script src="web-update\.js"><\/script>/.test(index), "index.html loads web-update.js");

/** A page served as `stamp`, the server running `running`. */
function tab({ stamp = "0.2.6-test.253", running = "0.2.6-test.254", desktop = false, reachable = true } = {}) {
  const listeners = {};
  const store = new Map();
  const page = {
    reloads: 0, intervals: 0, visibility: "visible", focus: null, dialog: false,
  };
  const document = {
    get visibilityState() { return page.visibility; },
    get activeElement() { return page.focus; },
    querySelector: q => q.startsWith("dialog") ? (page.dialog ? {} : null) : q.includes("?v=") ? (stamp ? { src: `app.js?v=${stamp}` } : null) : null,
    addEventListener: (t, f) => { (listeners[t] ||= []).push(f); },
  };
  const ctx = {
    document, location: { href: "https://teams.example/#/library", reload() {} }, URL,
    sessionStorage: { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)) },
    setInterval: () => { page.intervals++; }, Date,
    Server: { version: async () => { if (!reachable) throw new Error("offline"); return { version: running }; } },
    window: desktop ? { __TAURI__: {} } : {},
  };
  vm.createContext(ctx);
  vm.runInContext(`${code}\nthis.WebUpdate = WebUpdate;`, ctx);
  ctx.WebUpdate.reload = () => { page.reloads++; };
  const fire = t => (listeners[t] || []).forEach(f => f({}));
  return { page, w: ctx.WebUpdate, fire, store };
}

/** Lets the tab count as left alone: as if the last keypress was long ago. */
const later = (/** @type {number} */ ms) => { const real = Date.now; Date.now = () => real() + ms; return () => { Date.now = real; }; };

// Nothing done in the tab for a while: it loads the new page.
{
  const t = tab();
  const back = later(60_000);
  await t.w.look();
  back();
  assert.equal(t.page.reloads, 1, "a tab left alone loads the server's new version");
  assert.equal(t.w.loaded(), "0.2.6-test.253");
}

// Typing in it, a dialog open, or a click just now: it waits, then loads
// as soon as the person leaves the tab.
for (const [what, set] of /** @type {Array<[string, (p: any) => void]>} */ ([
  ["typing", p => { p.focus = { matches: () => true }; }],
  ["a dialog open", p => { p.dialog = true; }],
  ["a click just now", () => {}],
])) {
  const t = tab();
  set(t.page);
  t.fire("pointerdown");
  await t.w.look();
  assert.equal(t.page.reloads, 0, `not while ${what}`);
  assert.equal(t.w.settle(), false, `still not while ${what}`);
  t.page.visibility = "hidden";
  t.fire("visibilitychange");
  assert.equal(t.page.reloads, 1, `${what}: loads as soon as the person leaves the tab`);
}

// Once per version: still stamped old after loading, it does not load again.
{
  const t = tab();
  t.page.visibility = "hidden";
  await t.w.look();
  await t.w.look();
  assert.equal(t.page.reloads, 1, "one load per version");
}

// The same version, an unreachable server, the desktop app: nothing.
for (const [what, o] of /** @type {Array<[string, any]>} */ ([["the same version", { running: "0.2.6-test.253" }], ["the server unreachable", { reachable: false }]])) {
  const t = tab(o);
  t.page.visibility = "hidden";
  await t.w.look();
  assert.equal(t.page.reloads, 0, what);
}
{
  const t = tab({ desktop: true });
  assert.equal(t.page.intervals, 0, "the desktop app updates itself: the tab does not look");
  const w = tab();
  assert.equal(w.page.intervals, 2, "a browser tab looks now and then");
}
console.log("web update check passed");
