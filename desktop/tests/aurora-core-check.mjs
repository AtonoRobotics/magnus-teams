// AuroraCore unified 9-state engine and shared clock checks
// (decisions/2026-11-22-aurora-unified-state-and-contrast.md).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const code = readFileSync(join(here, "../ui/aurora-core.js"), "utf8");
const index = readFileSync(join(here, "../ui/index.html"), "utf8");
const assets = readFileSync(join(here, "../../crates/magnus/src/ui/assets.rs"), "utf8");

const corePos = index.indexOf('<script src="aurora-core.js"></script>');
assert.ok(corePos >= 0, "index.html loads aurora-core.js");
for (const dep of ["aurora.js", "aurora-avatar.js", "aurora-orb.js", "aurora-reasoning.js", "aurora-compact.js", "aurora-pages.js", "aurora-alerts.js"]) {
  const pos = index.indexOf(`<script src="${dep}"></script>`);
  assert.ok(pos > corePos, `index.html loads aurora-core.js before ${dep}`);
}
assert.ok(assets.includes('"aurora-core.js"'), "assets.rs embeds aurora-core.js");

function makeHarness({ style = "aurora", motion = "full" } = {}) {
  let nowMs = 10_000;
  /** @type {Set<string>} */
  let activeSelectors = new Set();
  /** @type {Array<{ isConnected: boolean, dataset: Record<string, string>, attrs: Set<string>, hasAttribute: (a: string) => boolean }>} */
  let pills = [];
  let hidden = false;
  let rafQueue = [];
  let nextTimerId = 1;
  const timers = new Map();
  const docListeners = new Map();
  const winListeners = new Map();

  const documentElement = { dataset: { style, motion } };
  const document = {
    documentElement,
    body: { id: "body" },
    get hidden() { return hidden; },
    querySelector(sel) {
      for (const s of activeSelectors) {
        if (sel.includes(s)) return { selector: s };
      }
      return null;
    },
    querySelectorAll(sel) {
      if (sel === ".aurora-reason") return pills;
      return [];
    },
    addEventListener(type, fn) {
      if (!docListeners.has(type)) docListeners.set(type, []);
      docListeners.get(type).push(fn);
    },
  };
  const windowObj = {
    setTimeout(fn, ms) { const id = nextTimerId++; timers.set(id, { fn, at: nowMs + ms }); return id; },
    clearTimeout(id) { timers.delete(id); },
    addEventListener(type, fn) {
      if (!winListeners.has(type)) winListeners.set(type, []);
      winListeners.get(type).push(fn);
    },
  };
  class FakeMutationObserver {
    constructor(cb) { this.cb = cb; }
    observe() {}
    disconnect() {}
  }
  class FakeElement {}
  const DateProxy = Object.assign(function FakeDate() { return new Date(nowMs); }, { now: () => nowMs });
  const ctx = {
    document,
    window: windowObj,
    globalThis: null,
    Theme: { stage: s => s === "aurora" || s === "obsidian-aurora" },
    Date: DateProxy,
    performance: { now: () => nowMs },
    requestAnimationFrame: fn => { rafQueue.push(fn); return rafQueue.length; },
    cancelAnimationFrame: () => { rafQueue = []; },
    clearTimeout: id => windowObj.clearTimeout(id),
    MutationObserver: FakeMutationObserver,
    Element: FakeElement,
    WeakMap,
    Set,
    Math,
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(code, ctx);
  return {
    core: ctx.AuroraCore,
    documentElement,
    setSelectors: list => { activeSelectors = new Set(list); },
    setPills: list => {
      pills = list.map(p => ({
        isConnected: p.isConnected !== false,
        dataset: { ...(p.dataset || {}) },
        hasAttribute: a => (p.attrs || []).includes(a),
      }));
      return pills;
    },
    advance(ms) {
      nowMs += ms;
      for (const [id, t] of [...timers.entries()]) {
        if (t.at <= nowMs) { timers.delete(id); t.fn(); }
      }
    },
    setHidden(v) {
      hidden = v;
      for (const fn of docListeners.get("visibilitychange") || []) fn();
    },
    fireTheme() {
      for (const fn of winListeners.get("magnus-theme") || []) fn();
    },
    tickFrame() {
      const q = rafQueue.splice(0);
      nowMs += 16;
      for (const fn of q) fn(nowMs);
    },
    pendingFrames: () => rafQueue.length,
  };
}

// 1. All 9 canonical states and BASE_OF mapping, including stage-tile/stage-decision waiting regression.
{
  const h = makeHarness();
  const seen = [];
  h.core.onState((s, b) => seen.push([s, b]));

  assert.equal(h.core.refreshState(), "ready");
  assert.equal(h.documentElement.dataset.auroraAvatarState, "ready");
  assert.equal(h.documentElement.dataset.auroraState, "idle");

  // Regression: .stage-tile[data-tone="needs"] and .stage-decision must put Aurora in waiting/needs.
  h.setSelectors(['.stage-tile[data-tone="needs"]']);
  assert.equal(h.core.refreshState(), "waiting");
  assert.equal(h.documentElement.dataset.auroraState, "needs");

  h.setSelectors([".stage-decision"]);
  assert.equal(h.core.refreshState(), "waiting");

  h.setSelectors(['.voice-in[aria-pressed="true"]']);
  assert.equal(h.core.refreshState(), "listening");
  assert.equal(h.documentElement.dataset.auroraState, "listening");

  h.setSelectors([".screen.held"]);
  assert.equal(h.core.refreshState(), "control");
  assert.equal(h.documentElement.dataset.auroraState, "takeover");

  // Thinking
  h.setSelectors(['.aurora-reason:is([data-phase="thinking"], [data-phase="writing"])']);
  assert.equal(h.core.refreshState(), "thinking");
  assert.equal(h.documentElement.dataset.auroraState, "thinking");

  // Working vs Planning
  const [livePill] = h.setPills([{ dataset: { phase: "working" }, attrs: [] }]);
  h.setSelectors(['.aurora-reason[data-phase="working"]']);
  assert.equal(h.core.refreshState(), "working");
  assert.equal(h.documentElement.dataset.auroraState, "executing");

  h.setPills([{ dataset: { phase: "working" }, attrs: ["data-planned"] }]);
  assert.equal(h.core.refreshState(), "planning");
  assert.equal(h.documentElement.dataset.auroraState, "executing");

  // Done hold & auto-decay back to ready on the same pill element.
  const [donePill] = h.setPills([{ dataset: { phase: "working" }, attrs: [] }]);
  h.setSelectors(['.aurora-reason[data-phase="working"]']);
  assert.equal(h.core.refreshState(), "working");
  h.setSelectors([]);
  donePill.dataset.phase = "done";
  assert.equal(h.core.refreshState(), "done");
  assert.equal(h.documentElement.dataset.auroraAvatarState, "done");
  h.advance(4100);
  assert.equal(h.core.state, "ready", "done decays back to ready after 4s");

  // Failed hold & auto-decay back to ready.
  const [failPill] = h.setPills([{ dataset: { phase: "working" }, attrs: [] }]);
  h.setSelectors(['.aurora-reason[data-phase="working"]']);
  assert.equal(h.core.refreshState(), "working");
  h.setSelectors([]);
  failPill.dataset.phase = "stopped";
  failPill.dataset.outcome = "failed";
  assert.equal(h.core.refreshState(), "failed");
  h.advance(4100);
  assert.equal(h.core.state, "failed", "failed still holds at 4.1s");
  h.advance(4100);
  assert.equal(h.core.state, "ready", "failed decays back to ready after 8s");

  assert.ok(seen.some(([s, b]) => s === "waiting" && b === "needs"));
  assert.ok(seen.some(([s, b]) => s === "listening" && b === "listening"));
  assert.ok(seen.some(([s, b]) => s === "control" && b === "takeover"));
  assert.ok(seen.some(([s, b]) => s === "planning" && b === "executing"));
  assert.ok(seen.some(([s, b]) => s === "done" && b === "idle"));
  assert.ok(seen.some(([s, b]) => s === "failed" && b === "idle"));
}

// 2. Shared rAF clock pauses when hidden and draws once when motion is reduced.
{
  const h = makeHarness({ style: "obsidian-aurora", motion: "full" });
  let frames = 0;
  const off = h.core.onFrame(() => { frames++; });
  assert.equal(h.pendingFrames(), 1);
  h.tickFrame();
  assert.equal(frames, 1);
  h.setHidden(true);
  assert.equal(h.pendingFrames(), 0, "clock pauses when tab is hidden");
  h.setHidden(false);
  assert.equal(h.pendingFrames(), 1, "clock resumes when tab returns");
  off();
  assert.equal(h.pendingFrames(), 0, "clock stops when last listener unsubscribes");
}

// 3. Reduced motion draws a single frame and schedules no loop.
{
  const h = makeHarness({ style: "aurora", motion: "reduced" });
  let frames = 0;
  h.core.onFrame(() => { frames++; });
  assert.equal(frames, 1, "reduced motion draws one still frame immediately");
  assert.equal(h.pendingFrames(), 0, "reduced motion schedules no animation loop");
}

// 4. Switching away from Aurora clears data-aurora-state and data-aurora-avatar-state.
{
  const h = makeHarness({ style: "aurora", motion: "full" });
  h.core.refreshState();
  assert.equal(h.documentElement.dataset.auroraAvatarState, "ready");
  h.documentElement.dataset.style = "obsidian-2";
  h.fireTheme();
  assert.equal(h.documentElement.dataset.auroraAvatarState, undefined);
  assert.equal(h.documentElement.dataset.auroraState, undefined);
}

console.log("aurora core check passed");
