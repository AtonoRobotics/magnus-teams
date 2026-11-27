// Reactive UIStore (Tier 3 Phase 1):
// Single source of truth for Magnus UI runtime state so AuroraCore,
// aurora-reasoning, Nova chrome, and mobile approval views subscribe to
// structured state transitions instead of scraping DOM classes or duplicating
// Server.tasks.get() polling loops.
"use strict";

/** How long a completed run holds the `done` phase before settling to `ready`. */
const STORE_DONE_HOLD_MS = 4000;
/** How long a failed run holds the `failed` phase before settling to `ready`. */
const STORE_FAILED_HOLD_MS = 8000;
/** How long a task snapshot in UIStore is considered fresh without re-fetching. */
const STORE_TASK_FRESH_MS = 4000;

const UIStore = (() => {
  /** @type {{
   *   route: string,
   *   asks: any[],
   *   tasksById: Record<string, any>,
   *   taskUpdatedAt: Record<string, number>,
   *   activeTaskId: ?string,
   *   liveWriting: Record<string, boolean>,
   *   takeoverHeld: boolean,
   *   voiceListening: boolean,
   *   killSwitch: any,
   *   connectedApps: string[],
   * }} */
  const state = {
    route: "",
    asks: [],
    tasksById: {},
    taskUpdatedAt: {},
    activeTaskId: null,
    liveWriting: {},
    takeoverHeld: false,
    voiceListening: false,
    killSwitch: null,
    connectedApps: [],
  };

  let hydrated = false;
  let ending = { state: "", until: 0 };
  /** @type {Map<string, Set<(val: any, full: typeof state) => void>>} */
  const listeners = new Map();
  /** @type {Set<(full: typeof state, changed: string[]) => void>} */
  const anyListeners = new Set();

  /** @param {string[]} keys */
  function emit(keys) {
    for (const k of keys) {
      const set = listeners.get(k);
      if (set) for (const fn of set) { try { fn(/** @type {any} */ (state)[k], state); } catch { /* isolate subscriber */ } }
    }
    for (const fn of anyListeners) { try { fn(state, keys); } catch { /* isolate subscriber */ } }
  }

  /** Read-only snapshot of current store state. */
  function get() { return state; }

  /** Whether live app data has been published into the store. */
  function hasState() { return hydrated; }

  /** Merge a partial state update and notify subscribers of changed keys.
   * @param {Partial<typeof state>} partial */
  function patch(partial) {
    if (!partial || typeof partial !== "object") return;
    const changed = [];
    for (const [k, v] of Object.entries(partial)) {
      if (/** @type {any} */ (state)[k] !== v) {
        /** @type {any} */ (state)[k] = v;
        changed.push(k);
      }
    }
    hydrated = true;
    if (changed.length) emit(changed);
  }

  /** Store or update a single task snapshot and track completion transitions.
   * @param {any} task */
  function setTask(task) {
    const id = task?.task_id || task?.id;
    if (!id) return;
    const prev = state.tasksById[id];
    const now = Date.now();
    state.tasksById = { ...state.tasksById, [id]: task };
    state.taskUpdatedAt[id] = now;
    hydrated = true;
    const prevStatus = prev?.status || "";
    const nextStatus = task.status || "";
    if (prevStatus && prevStatus !== nextStatus && ["running", "working", "queued", "suspended"].includes(prevStatus)) {
      if (nextStatus === "completed") ending = { state: "done", until: now + STORE_DONE_HOLD_MS };
      else if (nextStatus === "failed") ending = { state: "failed", until: now + STORE_FAILED_HOLD_MS };
    }
    emit(["tasksById"]);
  }

  /** Bulk-update tasks from a list response (e.g. GET /v1/tasks).
   * @param {any[]} list */
  function setTasks(list) {
    if (!Array.isArray(list)) return;
    const now = Date.now();
    const next = { ...state.tasksById };
    for (const t of list) {
      const id = t?.task_id || t?.id;
      if (!id) continue;
      const prev = next[id];
      if (prev && prev.status !== t.status && ["running", "working", "queued", "suspended"].includes(prev.status)) {
        if (t.status === "completed") ending = { state: "done", until: now + STORE_DONE_HOLD_MS };
        else if (t.status === "failed") ending = { state: "failed", until: now + STORE_FAILED_HOLD_MS };
      }
      next[id] = t;
      state.taskUpdatedAt[id] = now;
    }
    state.tasksById = next;
    hydrated = true;
    emit(["tasksById"]);
  }

  /** Mark whether a task is currently streaming live text over SSE.
   * @param {string} taskId @param {boolean} active */
  function setLiveWriting(taskId, active) {
    if (!taskId) return;
    const next = { ...state.liveWriting };
    if (active) next[taskId] = true; else delete next[taskId];
    state.liveWriting = next;
    hydrated = true;
    emit(["liveWriting"]);
  }

  /** @param {string} id */
  function getTask(id) { return id ? state.tasksById[id] || null : null; }

  /** Whether the cached snapshot for `id` is fresh enough to skip a redundant poll.
   * @param {string} id @param {number} [maxAgeMs] */
  function isTaskFresh(id, maxAgeMs = STORE_TASK_FRESH_MS) {
    const ts = state.taskUpdatedAt[id];
    return typeof ts === "number" && (Date.now() - ts) < maxAgeMs;
  }

  /** Subscribe to a single state slice key, or "*" for all changes. Returns unsubscribe fn.
   * @param {string} key @param {Function} fn */
  function subscribe(key, fn) {
    if (key === "*") {
      anyListeners.add(/** @type {any} */ (fn));
      return () => anyListeners.delete(/** @type {any} */ (fn));
    }
    let set = listeners.get(key);
    if (!set) { set = new Set(); listeners.set(key, set); }
    set.add(/** @type {any} */ (fn));
    return () => set.delete(/** @type {any} */ (fn));
  }

  /** Derive the canonical 9-state phase directly from store slices (no DOM scraping).
   * @param {number} [now] */
  function derivePhase(now = Date.now()) {
    if (state.voiceListening) return "listening";
    if (state.takeoverHeld) return "control";
    const tasks = Object.values(state.tasksById);
    const hasWaitingAsk = Array.isArray(state.asks) && state.asks.length > 0;
    const hasSuspended = tasks.some(t => t && (t.status === "suspended" || !!t.waiting_on));
    if (hasWaitingAsk || hasSuspended) return "waiting";
    const activeTasks = tasks.filter(t => t && (t.status === "running" || t.status === "working" || t.status === "queued"));
    if (activeTasks.length) {
      ending = { state: "", until: 0 };
      const hasPlan = activeTasks.some(t => t.plan && Array.isArray(t.plan.units) && t.plan.units.length > 0);
      return hasPlan ? "planning" : "working";
    }
    if (Object.keys(state.liveWriting).length > 0) {
      ending = { state: "", until: 0 };
      return "thinking";
    }
    if (ending.until > now && ending.state) return ending.state;
    return "ready";
  }

  /** Reset store (for tests). */
  function reset() {
    state.route = "";
    state.asks = [];
    state.tasksById = {};
    state.taskUpdatedAt = {};
    state.activeTaskId = null;
    state.liveWriting = {};
    state.takeoverHeld = false;
    state.voiceListening = false;
    state.killSwitch = null;
    state.connectedApps = [];
    hydrated = false;
    ending = { state: "", until: 0 };
  }

  return {
    get,
    hasState,
    patch,
    setTask,
    setTasks,
    setLiveWriting,
    getTask,
    isTaskFresh,
    subscribe,
    derivePhase,
    reset,
  };
})();
