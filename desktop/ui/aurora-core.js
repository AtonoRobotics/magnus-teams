// AuroraCore: the single runtime hub for Aurora and Obsidian 2 Aurora
// (decisions/2026-11-22-aurora-unified-state-and-contrast.md).
// - One canonical 9-state engine (ready, listening, thinking, planning,
//   working, waiting, control, done, failed) driving the avatar, ambient
//   canvas, particles and the composer's status line without contradicting
//   each other.
// - One shared requestAnimationFrame clock so the ambient wash, particle
//   field and avatar orbs stay phase-synchronized and pause when hidden or
//   outside full motion.
// - One coordinated MutationObserver hub so Aurora modules share a single
//   DOM watch instead of running separate subtree observers across the page.
"use strict";

/** How long the done state holds after a run ends well. */
const CORE_DONE_HOLD_MS = 4000;
/** How long the failed state holds after a run ends in failure. */
const CORE_FAILED_HOLD_MS = 8000;
/** Extra margin after an ending hold before re-checking state. */
const CORE_DECAY_PADDING_MS = 40;
/** Coalescing delay before re-evaluating state after DOM mutations. */
const CORE_STATE_DEBOUNCE_MS = 45;

const AuroraCore = (() => {
  const BASE_OF = {
    ready: "idle",
    listening: "listening",
    thinking: "thinking",
    planning: "executing",
    working: "executing",
    waiting: "needs",
    control: "takeover",
    done: "idle",
    failed: "idle",
  };

  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";

  let state = "ready";
  /** @type {{ state: string, until: number }} */
  let ending = { state: "", until: 0 };
  /** @type {WeakMap<Element, string>} */
  const pillSeen = new WeakMap();
  let decayTimer = 0;

  /** @type {Set<(state: string, base: string) => void>} */
  const stateListeners = new Set();
  /** @type {Set<(now: number, dt: number) => void>} */
  const frameListeners = new Set();
  /** @type {Set<(records: MutationRecord[]) => void>} */
  const domListeners = new Set();
  /** @type {Set<() => void>} */
  const mainListeners = new Set();

  /** Reads the canonical 9-state status from UIStore (or DOM fallback in isolated tests). */
  function detect() {
    if (document.querySelector('.voice-in[aria-pressed="true"]')) return "listening";
    if (document.querySelector('.state-pill[data-pill="mine"], .screen.held')) return "control";
    if (typeof UIStore !== "undefined" && UIStore.hasState()) return UIStore.derivePhase();
    if (document.querySelector('.st.need, .stage-tile[data-tone="needs"], .stage-decision, .detail-body[data-panel="needs"] .ask-answers, .aurora-reason[data-phase="needs"]')) return "waiting";

    const pills = [...document.querySelectorAll(".aurora-reason")].filter(p => p.isConnected);
    const last = /** @type {HTMLElement|undefined} */ (pills[pills.length - 1]);
    const phase = last?.dataset.phase || "";
    const now = Date.now();
    if (last) {
      const before = pillSeen.get(last);
      pillSeen.set(last, phase);
      const bad = phase === "stopped" && last.dataset.outcome === "failed";
      if (before && before !== phase && (phase === "done" || bad)) {
        const hold = bad ? CORE_FAILED_HOLD_MS : CORE_DONE_HOLD_MS;
        ending = { state: bad ? "failed" : "done", until: now + hold };
        clearTimeout(decayTimer);
        decayTimer = window.setTimeout(refreshState, hold + CORE_DECAY_PADDING_MS);
      }
    }

    const busyWork = !!document.querySelector('.st.working, .comp-dot.run, .tcard[data-status="running"], .tcard[data-status="working"], .state-pill[data-pill="working"], .state-pill[data-pill="waking"], .aurora-reason[data-phase="working"]');
    const busyThink = !!document.querySelector('.aurora-reason:is([data-phase="thinking"], [data-phase="writing"])');
    if (busyWork || busyThink) ending = { state: "", until: 0 };
    if (busyWork) return last?.hasAttribute("data-planned") ? "planning" : "working";
    if (busyThink) return "thinking";
    if (ending.until > now) return ending.state;
    return "ready";
  }

  /** Re-evaluates state and notifies subscribers when it changes. */
  function refreshState() {
    if (!on()) {
      delete document.documentElement.dataset.auroraState;
      delete document.documentElement.dataset.auroraAvatarState;
      state = "ready";
      return state;
    }
    const next = detect();
    const base = BASE_OF[/** @type {keyof typeof BASE_OF} */ (next)] || "idle";
    if (document.documentElement.dataset.auroraState !== base) document.documentElement.dataset.auroraState = base;
    if (document.documentElement.dataset.auroraAvatarState !== next) document.documentElement.dataset.auroraAvatarState = next;
    if (next !== state) {
      state = next;
      for (const fn of stateListeners) fn(next, base);
    }
    return state;
  }

  // ---------- Shared animation clock ----------
  let rafId = 0;
  let lastTs = 0;

  function step(/** @type {number} */ now) {
    rafId = 0;
    if (document.hidden || !on() || !frameListeners.size) return;
    const dt = lastTs ? Math.min((now - lastTs) / 1000, 0.05) : 0.016;
    lastTs = now;
    for (const fn of frameListeners) fn(now, dt);
    if (moving() && frameListeners.size) rafId = requestAnimationFrame(step);
  }

  function wake() {
    if (!on() || document.hidden || !frameListeners.size) return;
    if (moving()) {
      if (!rafId) { lastTs = performance.now(); rafId = requestAnimationFrame(step); }
    } else {
      if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
      const now = performance.now();
      for (const fn of frameListeners) fn(now, 0);
    }
  }

  function stopClock() {
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
  }

  /** @param {(now: number, dt: number) => void} fn */
  function onFrame(fn) {
    frameListeners.add(fn);
    wake();
    return () => { frameListeners.delete(fn); if (!frameListeners.size) stopClock(); };
  }

  /** @param {(state: string, base: string) => void} fn */
  function onState(fn) {
    stateListeners.add(fn);
    return () => { stateListeners.delete(fn); };
  }

  /** @param {(records: MutationRecord[]) => void} fn */
  function onDom(fn) {
    domListeners.add(fn);
    return () => { domListeners.delete(fn); };
  }

  /** @param {() => void} fn */
  function onMain(fn) {
    mainListeners.add(fn);
    return () => { mainListeners.delete(fn); };
  }

  // ---------- Single coordinated MutationObserver ----------
  let stateQueued = 0;
  let mainQueued = 0;
  function queueState() {
    if (stateQueued) return;
    stateQueued = window.setTimeout(() => { stateQueued = 0; refreshState(); }, CORE_STATE_DEBOUNCE_MS);
  }
  function queueMain() {
    if (mainQueued) return;
    mainQueued = requestAnimationFrame(() => {
      mainQueued = 0;
      for (const fn of mainListeners) fn();
    });
  }

  let bodyObs = null;
  function attach() {
    if (bodyObs || !document.body) return;
    bodyObs = new MutationObserver(records => {
      if (!on()) return;
      for (const fn of domListeners) fn(records);
      const mainChanged = records.some(r => {
        const el = r.target instanceof Element ? r.target : null;
        return el && (el.id === "main" || el.matches(".shell") || el === document.body);
      });
      if (mainChanged) queueMain();
      queueState();
    });
    bodyObs.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["class", "data-pill", "data-status", "aria-pressed", "data-phase", "data-tone", "data-outcome", "data-planned"],
    });
    refreshState();
    wake();
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) stopClock();
    else { refreshState(); wake(); }
  });
  window.addEventListener("hashchange", () => { if (on()) { queueMain(); queueState(); } });
  window.addEventListener("magnus-theme", () => {
    if (on()) { refreshState(); wake(); queueMain(); }
    else { stopClock(); refreshState(); }
  });
  if (typeof UIStore !== "undefined") UIStore.subscribe("*", () => { if (on()) queueState(); });

  if (document.body) attach();
  else document.addEventListener("DOMContentLoaded", attach);

  return {
    get state() { return state; },
    get base() { return BASE_OF[/** @type {keyof typeof BASE_OF} */ (state)] || "idle"; },
    BASE_OF,
    on,
    moving,
    detect,
    refreshState,
    onState,
    onFrame,
    onDom,
    onMain,
    wake,
  };
})();
globalThis.AuroraCore = AuroraCore;
