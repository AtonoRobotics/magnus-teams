// Aurora says what Magnus is doing where the person is looking, as the
// mockup's agent demo does (decisions/2026-11-21-aurora-reasoning.md):
// - an answer being written carries the mockup's reasoning pill instead of a
//   still "Working on an answer…": the step it is on as its words, the steps
//   lighting up and ticking off below, and when the answer comes it folds to
//   "Completed 3 steps · 12s" above it;
// - above the composer, a line says the state its conversation is in
//   ("Ready · standing by", "Working…"), in that state's colour;
// - the composer grows and shrinks smoothly as the person types;
// - what the app would say in a toast ("Allowed.", "Saved.") is said in that
//   line for as long as the toast would show (`tell`), so nothing floats
//   over the stage (decisions/2026-11-21-aurora-alerts.md).
// The pill is one node per answer, kept across the thread's repaints and
// changed in place, so its changes move instead of being redrawn.
"use strict";

/** How often a running answer's task is read for its steps. */
const REASON_POLL_MS = 1500;
/** How long the composer takes to grow or shrink by a line. */
const SAY_GROW_MS = 180;
/** How many answers' pills are remembered in this session. */
const REASON_KEPT_MAX = 200;

const AuroraReasoning = (() => {
  const ENDS = ["completed", "failed", "stopped", "cancelled"];
  const SAY_WORDS = {
    ready: "Ready · standing by",
    idle: "Ready · standing by",
    listening: "Listening · speak naturally…",
    thinking: "Thinking & planning…",
    planning: "Planning next steps…",
    working: "Working…",
    executing: "Working…",
    control: "You're in control",
    takeover: "You're in control",
    waiting: "Needs you · a decision is waiting",
    needs: "Needs you · a decision is waiting",
    done: "Done · ready for your review",
    failed: "Stopped with a problem",
  };
  const CHEVRON = '<svg class="aurora-reason-chev" viewBox="0 0 24 24" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>';

  /**
   * One answer's pill, by its entry's id.
   * @typedef {{ task: string, el: HTMLElement, steps: Array<{ text: string, state: string }>, phase: string, started: number, ended: number, open: boolean, chosen: boolean, loading: boolean, outcome?: string }} Reason
   * @type {Map<string, Reason>}
   */
  const kept = new Map();
  let timer = 0;

  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";

  /** @param {number} ms */
  function lasted(ms) {
    const s = Math.max(1, Math.round(ms / 1000));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  }

  /** @param {Reason} r */
  function summary(r) {
    const active = r.steps.find(s => s.state === "active");
    const n = r.steps.length;
    switch (r.phase) {
      case "needs": return "Waiting on your decision";
      case "working": return active ? active.text : "Working…";
      case "writing": return "Writing the answer…";
      case "done": return n ? `Completed ${n} ${n === 1 ? "step" : "steps"} · ${lasted(r.ended - r.started)}` : `Answered in ${lasted(r.ended - r.started)}`;
      case "stopped": return n ? `Stopped after ${n} ${n === 1 ? "step" : "steps"}` : "Stopped";
      default: return "Thinking & planning…";
    }
  }

  /** @param {string} id @param {string} task @returns {Reason} */
  function make(id, task) {
    const el = document.createElement("div");
    el.className = "aurora-reason";
    el.innerHTML = `<button type="button" class="aurora-reason-toggle" aria-expanded="true"><span class="aurora-reason-spark" aria-hidden="true"></span><span class="aurora-reason-summary" aria-live="polite"></span>${CHEVRON}</button><div class="aurora-reason-body"><ol class="aurora-reason-steps"></ol></div>`;
    /** @type {Reason} */
    const r = { task, el, steps: [], phase: "thinking", started: Date.now(), ended: 0, open: true, chosen: false, loading: false };
    el.querySelector(".aurora-reason-toggle")?.addEventListener("click", () => { r.open = !r.open; r.chosen = true; draw(r); });
    if (kept.size >= REASON_KEPT_MAX) kept.delete(kept.keys().next().value);
    kept.set(id, r);
    return r;
  }

  /** Changes the pill in place to what `r` says. @param {Reason} r */
  function draw(r) {
    const el = r.el;
    el.dataset.phase = r.phase;
    // For the avatar (aurora-avatar.js): how the run ended, and whether its
    // plan is laid out with nothing started yet.
    if (r.outcome) el.dataset.outcome = r.outcome;
    el.toggleAttribute("data-planned", r.phase === "working" && r.steps.length > 0 && r.steps.every(s => s.state === "todo"));
    el.toggleAttribute("data-open", r.open && r.steps.length > 0);
    el.toggleAttribute("data-steps", r.steps.length > 0);
    el.querySelector(".aurora-reason-toggle")?.setAttribute("aria-expanded", String(r.open && r.steps.length > 0));
    const said = el.querySelector(".aurora-reason-summary"), words = summary(r);
    if (said && said.textContent !== words) said.textContent = words;
    const list = el.querySelector(".aurora-reason-steps");
    if (!list) return;
    r.steps.forEach((s, i) => {
      let li = /** @type {HTMLElement|undefined} */ (list.children[i]);
      if (!li) {
        li = document.createElement("li");
        li.innerHTML = '<span class="aurora-reason-step"></span><span class="aurora-reason-check" aria-hidden="true">✓</span>';
        // A step enters once: put back after a repaint, it does not replay.
        if (moving()) { li.classList.add("aurora-reason-in"); li.addEventListener("animationend", () => li?.classList.remove("aurora-reason-in"), { once: true }); }
        list.appendChild(li);
      }
      li.dataset.state = s.state;
      const t = li.firstElementChild;
      if (t && t.textContent !== s.text) t.textContent = s.text;
    });
    while (list.children.length > r.steps.length) list.lastElementChild?.remove();
  }

  /**
   * What the task says now: its plan's steps, or each thing it was doing
   * (its status line) as a step of its own.
   * @param {Reason} r @param {any} t a task from GET /v1/tasks/{id}
   */
  function learn(r, t) {
    const at = Date.parse(t.created_at || "");
    if (!Number.isNaN(at)) r.started = at;
    const units = (t.plan && Array.isArray(t.plan.units) ? t.plan.units : []).filter((/** @type {any} */ u) => u.status !== "dropped" && u.goal);
    if (units.length) {
      r.steps = units.map((/** @type {any} */ u) => ({ text: String(u.goal).trim(), state: u.status === "done" ? "done" : u.status === "active" ? "active" : "todo" }));
    } else if (t.status_line) {
      const last = r.steps[r.steps.length - 1];
      if (!last || last.text !== t.status_line) {
        r.steps.forEach(s => { s.state = "done"; });
        r.steps.push({ text: t.status_line, state: "active" });
      }
    } else {
      r.steps.forEach(s => { s.state = "done"; });
    }
    if (ENDS.includes(t.status)) { r.outcome = t.status; return end(r, t.status === "completed"); }
    const writing = !!document.querySelector(`[data-live="${CSS.escape(r.task)}"]`)?.textContent?.trim();
    r.phase = t.status === "suspended" || t.waiting_on ? "needs" : r.steps.some(s => s.state === "active") ? "working" : writing ? "writing" : r.steps.length ? "working" : "thinking";
  }

  /** @param {Reason} r @param {boolean} answered */
  function end(r, answered) {
    if (r.phase === "done" || r.phase === "stopped") return;
    r.steps.forEach(s => { if (s.state !== "done") s.state = answered ? "done" : s.state; });
    r.phase = answered ? "done" : "stopped";
    r.ended = Date.now();
    if (!r.chosen) r.open = false;
  }

  /** Reads each running answer's task, reusing fresh UIStore snapshots to avoid duplicate polling. */
  async function poll() {
    if (!on() || typeof Server === "undefined" || !Server.tasks?.get) return;
    for (const r of kept.values()) {
      if (r.loading || r.phase === "done" || r.phase === "stopped" || !r.el.isConnected) continue;
      if (typeof UIStore !== "undefined" && UIStore.isTaskFresh(r.task)) {
        const cached = UIStore.getTask(r.task);
        if (cached) { learn(r, cached); draw(r); continue; }
      }
      r.loading = true;
      try {
        const t = await Server.tasks.get(r.task);
        if (typeof UIStore !== "undefined" && t) UIStore.setTask(t);
        learn(r, t);
        draw(r);
      } catch { /* the next read tries again */ }
      finally { r.loading = false; }
    }
  }

  /**
   * Puts the pill back where a repaint left room for it. Its style is taken
   * as it was before anything changes, so what changes next (the fold when
   * the answer comes) moves instead of being drawn finished.
   * @param {HTMLElement} el @param {Element} parent @param {?Node} before
   */
  function put(el, parent, before) {
    parent.insertBefore(el, before);
    void el.offsetHeight;
  }

  /** Puts each answer's pill where the thread now has it. */
  function mount() {
    if (!on()) return;
    let running = false;
    document.querySelectorAll('.entry[data-entry]:is([data-kind="turn"], [data-kind="answer"])').forEach(entry => {
      const id = /** @type {HTMLElement} */ (entry).dataset.entry || "";
      const live = entry.querySelector("[data-live]");
      let r = kept.get(id);
      if (live) {
        r = r || make(id, /** @type {HTMLElement} */ (live).dataset.live || "");
        running = true;
        const msg = live.closest(".msg") || entry;
        if (r.el.parentElement !== msg) put(r.el, msg, msg.firstChild);
      } else if (r) {
        // The answer came, the turn ended without one, or it waits on the person.
        if (entry.getAttribute("data-kind") === "answer") end(r, true);
        else if (entry.querySelector(".msg > .bad")) { r.outcome = r.outcome || "failed"; end(r, false); }
        else if (r.phase !== "done" && r.phase !== "stopped") r.phase = "needs";
        const body = entry.querySelector(".answer-body"), msg = entry.querySelector(".msg") || entry;
        const parent = body ? body.parentElement || entry : msg;
        const before = body || msg.firstChild;
        if (r.el.parentElement !== parent || r.el.nextSibling !== before) put(r.el, parent, before);
      } else {
        return;
      }
      draw(r);
    });
    if (running && !timer) { timer = setInterval(poll, REASON_POLL_MS); poll(); }
    else if (!running && timer) { clearInterval(timer); timer = 0; }
    say();
  }

  /**
   * The state the line above a composer speaks of: the person's own
   * (listening, in control of the computer), or else this conversation's
   * work only. Work running elsewhere has its own card and tile; under an
   * answered conversation the line said "Working…" for it (2026-11-28, the
   * installed app on test.427: "pong", answered in 3s, still "working").
   */
  function threadState() {
    const own = document.documentElement.dataset.auroraAvatarState || "";
    if (own === "listening" || own === "control") return own;
    const thread = document.getElementById("thread");
    if (!thread) return "ready";
    if (thread.querySelector('.st.need, .aurora-reason[data-phase="needs"]')) return "waiting";
    const pills = thread.querySelectorAll(".aurora-reason");
    const last = /** @type {HTMLElement|undefined} */ (pills[pills.length - 1]);
    const phase = last?.dataset.phase || "";
    if (phase === "working" || thread.querySelector('.st.working, .tcard:is([data-status="running"], [data-status="working"])')) return last?.hasAttribute("data-planned") ? "planning" : "working";
    if (phase === "thinking" || phase === "writing") return "thinking";
    return "ready";
  }

  /** The line above each composer saying the state this conversation is in. */
  function say() {
    const key = threadState();
    const words = SAY_WORDS[/** @type {keyof typeof SAY_WORDS} */ (key)] || SAY_WORDS.ready;
    document.querySelectorAll(".composer-slot > .say, .composer-slot > form.say").forEach(form => {
      const slot = form.parentElement;
      if (!slot) return;
      let line = slot.querySelector(":scope > .aurora-say-state");
      if (!line) {
        line = document.createElement("div");
        line.className = "aurora-say-state";
        line.innerHTML = '<span class="aurora-state-dot" aria-hidden="true"></span><span class="aurora-say-text" aria-hidden="true"></span><span class="aurora-say-told" role="status"></span>';
        slot.insertBefore(line, form);
      }
      const text = line.querySelector(".aurora-say-text");
      if (text && text.textContent !== words) text.textContent = words;
      const state = SAY_WORDS[/** @type {keyof typeof SAY_WORDS} */ (key)] ? key : "ready";
      if (/** @type {HTMLElement} */ (line).dataset.state !== state) /** @type {HTMLElement} */ (line).dataset.state = state;
    });
  }

  /**
   * Says what a toast would, in the composer's line, for `ms`.
   * @param {string} msg @param {number} ms
   * @returns {boolean} whether it was said here (no line shown: no)
   */
  function tell(msg, ms) {
    if (!on()) return false;
    const line = /** @type {HTMLElement|undefined} */ ([...document.querySelectorAll(".aurora-say-state")].find(l => /** @type {HTMLElement} */ (l).offsetParent !== null));
    const told = line?.querySelector(".aurora-say-told");
    if (!line || !told) return false;
    told.textContent = msg;
    line.setAttribute("data-told", "");
    clearTimeout(Number(line.dataset.toldTimer));
    line.dataset.toldTimer = String(setTimeout(() => { line.removeAttribute("data-told"); told.textContent = ""; }, ms));
    return true;
  }

  /** Leaves the page as the other styles have it. */
  function off() {
    document.querySelectorAll(".aurora-reason, .aurora-say-state").forEach(el => el.remove());
    if (timer) { clearInterval(timer); timer = 0; }
  }

  // The composer's own listener sets its new height; the height it is drawn
  // at is taken first (capture), and one change per frame plays before the
  // frame shows, from the height drawn to the height the composer set.
  /** @type {WeakSet<HTMLElement>} */
  const growing = new WeakSet();
  document.addEventListener("input", e => {
    const box = /** @type {HTMLElement} */ (e.target);
    if (!on() || !moving() || !(box instanceof HTMLTextAreaElement) || !box.closest(".say") || growing.has(box)) return;
    growing.add(box);
    // A grow still playing is stopped first: the composer measures its text
    // at the height it is drawn at, and would take the animated one.
    const from = box.offsetHeight;
    box.getAnimations().forEach(a => a.cancel());
    requestAnimationFrame(() => {
      growing.delete(box);
      const to = parseFloat(box.style.height) || box.offsetHeight;
      if (Math.abs(to - from) >= 1) box.animate([{ height: `${from}px` }, { height: `${to}px` }], { duration: SAY_GROW_MS, easing: "cubic-bezier(.22,1,.36,1)" });
    });
  }, true);

  let queued = 0;
  const soon = () => { if (!queued) queued = requestAnimationFrame(() => { queued = 0; if (on()) mount(); else off(); }); };
  function init() {
    if (typeof UIStore !== "undefined") {
      UIStore.subscribe("tasksById", tasks => {
        if (!on()) return;
        for (const r of kept.values()) {
          const t = tasks[r.task];
          if (t && r.el.isConnected && r.phase !== "done" && r.phase !== "stopped") { learn(r, t); draw(r); }
        }
      });
    }
    if (typeof AuroraCore !== "undefined") {
      AuroraCore.onDom(soon);
      AuroraCore.onState(() => { if (on()) say(); });
    }
    window.addEventListener("magnus-theme", soon);
    window.addEventListener("magnus-render", soon);
    soon();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();

  return { mount, poll, on, kept, tell, threadState };
})();
