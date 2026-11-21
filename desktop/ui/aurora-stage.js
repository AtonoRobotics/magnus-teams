// The Aurora stage: in a style that has one (Theme.stage), the conversation
// and its cards work as the mockup's agent demo does
// (decisions/2026-11-20-aurora-stage.md).
// - The conversation is the main card. Under it is the bento: a box appears
//   while something is relevant and leaves when it is not, and the others
//   move to their new places. Live Workspace while work in this conversation
//   runs (with the beam and what it is doing), waits on the person (amber) or
//   has just finished (mint, with its result); Approvals while a decision
//   waits; Up Next while a routine is due soon; Files while a new file is
//   fresh; Apps while one needs attention; the computer while it works or
//   asks for the person; Recent Work while work elsewhere has just finished.
// - Every view stays one click away in the conversation's head: a row of the
//   panel's icons (Approvals, Tasks, Routines, Rules, Files, Apps).
// - There is no panel beside the conversation. Anything opened is the
//   artifact canvas: it slides in on the right, and the conversation and its
//   boxes narrow and move over. When work the person is watching finishes or
//   needs them, its result slides open on its own. Close slides it out.
// - A new block of an answer streams in word by word.
"use strict";

const AuroraStage = (() => {
  const FILES_FRESH_MS = 30_000;
  const RECENT_MS = 60 * 60_000;
  const DUE_SOON_MS = 24 * 60 * 60_000;
  const MOVE_MS = 460;
  const LEAVE_MS = 320;
  const JUMP_PX = 24;
  const WORD_STEP_MS = 34;
  const WORDS_STREAMED = 120;
  const SHOWN_BOXES = 4;
  const MOVE_EASE = "cubic-bezier(0.16, 1, 0.3, 1)";
  const html = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  let files = { at: 0, list: [], loading: null };
  let lastArtifact = null;
  /** @type {Map<string, string>|null} each task's state when last seen */
  let seen = null;

  /** Whether the shown style draws the stage. */
  function on() {
    return typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  }

  // The person's latest files, read at most every FILES_FRESH_MS.
  async function recentFiles() {
    if (Date.now() - files.at < FILES_FRESH_MS) return files.list;
    if (!files.loading && typeof Server !== "undefined" && Server.files?.list) {
      files.loading = Server.files.list({}).then(r => { files = { at: Date.now(), list: (r && r.files) || [], loading: null }; return files.list; }).catch(() => { files.loading = null; return files.list; });
    }
    return files.loading || files.list;
  }

  const ended = t => ENDED.includes(t.status);
  const waits = t => !ended(t) && needsYou(t);
  const stateOfTask = t => (ended(t) ? t.status : waits(t) ? "needs" : "running");
  const stepOf = t => ((t.plan && t.plan.units) || []).find(u => u.status === "active");
  const named = t => t.title || firstLine(t.text) || "Task";
  const age = iso => { const at = Date.parse(iso || ""); return Number.isNaN(at) ? Infinity : Date.now() - at; };
  const latest = list => list.sort((a, b) => (b.ended_at || b.created_at || "").localeCompare(a.ended_at || a.created_at || ""))[0];
  const ATTENTION = ["needs_attention", "needs_more_access", "connecting"];

  /**
   * What is relevant now, most urgent first: each a box of the bento.
   * @returns {Promise<Array<{ key: string, label: string, icon: string, svg?: string, open: string, href: string, tone: string, title: string, desc: string, count?: number }>>}
   */
  async function boxes() {
    const tasks = App.tasks || [];
    const here = tasks.filter(t => App.thread && t.thread === App.thread);
    const run = here.filter(t => !ended(t)).find(waits) || here.find(t => !ended(t));
    const last = latest(here.filter(t => ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    const list = [];
    const live = { key: "live", label: "Live Workspace", icon: "sparkles" };
    if (run) {
      const step = stepOf(run);
      list.push({ ...live, tone: waits(run) ? "needs" : "run", open: `task:${run.task_id}`, href: `#/task/${enc(run.task_id)}`, title: named(run), desc: waits(run) ? "Waiting on you to decide" : run.status_line || (step && step.goal) || "Working…" });
    } else if (last) {
      list.push({ ...live, tone: last.status === "completed" ? "done" : "idle", open: `task:${last.task_id}`, href: `#/task/${enc(last.task_id)}`, title: named(last), desc: firstLine(last.summary) || (last.status === "completed" ? "Done" : "Stopped") });
    }
    const pending = needsMe();
    if (pending.length) list.push({ key: "approvals", label: "Approvals", icon: "need", href: "#/inbox", tone: "needs", count: pending.length, open: pending.length === 1 && pending[0].id ? `item:${pending[0].id}` : "needs",
      title: pending[0].summary || pending[0].ask?.sentence || "A decision", desc: pending.length > 1 ? `${pending.length - 1} more waiting on you` : "Nothing goes out until you approve." });
    const comp = (App.computers || []).find(x => x.owner?.person === App.me?.id);
    const w = comp && typeof controlWords === "function" ? controlWords(comp, false) : null;
    if (comp && w && ["working", "asks", "waking"].includes(w.key)) {
      const svg = typeof UI !== "undefined" && UI.computer ? `<svg class="ic" viewBox="0 0 24 24">${UI.computer}</svg>` : "";
      list.push({ key: "computer", label: "Computer", icon: "globe", svg, tone: w.key === "asks" ? "needs" : "run", open: `computer:${comp.id}`, href: `#/computer/${enc(comp.id)}`, title: "Cloud Computer", desc: w.pill });
    }
    const due = (App.schedules || []).filter(sc => !sc.ended && !sc.paused && sc.next && -age(sc.next) < DUE_SOON_MS).sort((a, b) => String(a.next).localeCompare(String(b.next)));
    if (due.length) list.push({ key: "next", label: "Up Next", icon: "calendar", tone: "idle", open: `schedule:${due[0].id}`, href: "#/schedules", title: due[0].title, desc: `${until(due[0].next)}${due.length > 1 ? ` · ${due.length - 1} more today` : ""}` });
    const fresh = (await recentFiles()).filter(f => age(f.at) < RECENT_MS);
    if (fresh.length) list.push({ key: "files", label: "Files", icon: "file", tone: "done", open: `file:${fresh[0].id}`, href: "#/files", title: fresh[0].name, desc: fresh.length > 1 ? `${plural(fresh.length, "new file")} · ${when(fresh[0].at)}` : `New · ${when(fresh[0].at)}` });
    const connectors = (typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : App.connectors) || [];
    const asking = connectors.filter(c => ATTENTION.includes(stateOf(c)));
    if (asking.length) list.push({ key: "apps", label: "Apps", icon: "grid", tone: "needs", open: `connector:${asking[0].name}`, href: "#/apps", title: asking[0].title, desc: asking.length > 1 ? `${asking.length} apps need attention` : "Needs attention to keep working" });
    const elsewhere = latest(tasks.filter(t => t.thread !== App.thread && ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    if (elsewhere) list.push({ key: "recent", label: "Recent Work", icon: "clock", tone: elsewhere.status === "completed" ? "done" : "idle", open: `task:${elsewhere.task_id}`, href: "#/tasks", title: named(elsewhere), desc: `${elsewhere.status === "completed" ? "Done" : "Stopped"} · ${when(elsewhere.ended_at || elsewhere.created_at)}` });
    const rank = c => (c.tone === "needs" ? 0 : c.tone === "run" ? 1 : 2);
    return list.map((c, i) => [c, i]).sort((a, b) => rank(a[0]) - rank(b[0]) || a[1] - b[1]).map(([c]) => c).slice(0, SHOWN_BOXES);
  }

  /** A box's inside, as the mockup's small bento box. */
  function boxInner(c) {
    return `<span class="stage-tile-label"><span class="stage-tile-ic" aria-hidden="true">${c.svg || icon(c.icon)}</span><span class="stage-tile-name">${html(c.label)}</span>${c.count ? `<span class="stage-tile-badge">${c.count}</span>` : ""}<span class="mini-dot" aria-hidden="true"></span></span>
      <strong class="stage-tile-title">${html(c.title)}</strong>
      <span class="stage-tile-desc">${html(c.desc)}</span>`;
  }

  // Where each box is, so a change can slide them from there.
  /** @type {WeakMap<Element, DOMRect>} */
  let at = new WeakMap();
  const boxesIn = bento => [...bento.querySelectorAll(":scope > .stage-tile:not(.stage-tile-leaving)")];
  function remember(bento) { boxesIn(bento).forEach(b => at.set(b, b.getBoundingClientRect())); }
  const moving = () => document.documentElement.dataset.motion === "full";
  function slide(bento, min) {
    if (!moving()) return remember(bento);
    for (const b of boxesIn(bento)) {
      const from = at.get(b), to = b.getBoundingClientRect();
      at.set(b, to);
      if (!from || b.classList.contains("stage-tile-enter")) continue;
      const dx = from.left - to.left, dy = from.top - to.top;
      if (Math.abs(dx) < min && Math.abs(dy) < min) continue;
      b.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: MOVE_MS, easing: MOVE_EASE });
    }
  }

  /**
   * The bento shows exactly these boxes: new ones come in, gone ones leave
   * from where they were, and the rest slide to their new places.
   * @param {Element} bento @param {Awaited<ReturnType<typeof boxes>>} list
   */
  function place(bento, list) {
    remember(bento);
    const want = new Set(list.map(c => c.key));
    const box = bento.getBoundingClientRect();
    for (const b of boxesIn(bento)) {
      if (want.has(b.dataset.tile)) continue;
      if (!moving()) { b.remove(); continue; }
      const r = b.getBoundingClientRect();
      Object.assign(b.style, { position: "absolute", left: `${r.left - box.left}px`, top: `${r.top - box.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      b.classList.add("stage-tile-leaving");
      setTimeout(() => b.remove(), LEAVE_MS);
    }
    for (const c of list) {
      let b = bento.querySelector(`:scope > .stage-tile[data-tile="${c.key}"]:not(.stage-tile-leaving)`);
      if (!b) {
        b = document.createElement("a");
        b.dataset.tile = c.key;
        if (moving()) { b.classList.add("stage-tile-enter"); b.addEventListener("animationend", () => b.classList.remove("stage-tile-enter"), { once: true }); }
      }
      b.classList.add("stage-tile");
      b.classList.toggle("aurora-executing", c.tone === "run");
      b.dataset.tone = c.tone;
      b.dataset.open = c.open;
      b.setAttribute("href", c.href);
      const inner = boxInner(c);
      if (b._html !== inner) { b.innerHTML = inner; b._html = inner; }
      bento.append(b);
    }
    bento.querySelectorAll(":scope > .stage-tile-leaving").forEach(b => bento.append(b));
    bento.dataset.count = String(list.length);
    slide(bento, 1);
  }

  /** The conversation head's row of the panel's icons: every view, always. */
  function dock() {
    const n = needsMe().length;
    const views = [["needs", "#/inbox", "need", "Approvals"], ["tasks", "#/tasks", "clock", "Tasks"], ["schedules", "#/schedules", "calendar", "Routines"], ["rules", "#/rules", "shield", "Rules"], ["files", "#/files", "file", "Files"], ["apps", "#/apps", "grid", "Apps"]];
    return views.map(([open, href, ic, label]) => `<a class="icon-btn stage-dock-btn" href="${href}" data-open="${open}" title="${label}" aria-label="${label}${open === "needs" && n ? `, ${n} waiting` : ""}">${icon(ic)}${open === "needs" && n ? `<span class="stage-dock-badge" aria-hidden="true">${n}</span>` : ""}</a>`).join("");
  }

  // Work the person is watching that just finished or now needs them opens
  // its result beside the conversation, unless they opened something else.
  function follow() {
    const tasks = App.tasks || [];
    const now = new Map(tasks.map(t => [t.task_id, stateOfTask(t)]));
    const was = seen;
    seen = now;
    if (!was || !App.thread) return;
    const turned = tasks.find(t => t.thread === App.thread && was.get(t.task_id) === "running" && ["completed", "needs"].includes(now.get(t.task_id) || ""));
    if (!turned || (Panel.what && !Panel.autoOpened) || Panel.what === `task:${turned.task_id}`) return;
    openPanel(`task:${turned.task_id}`, { auto: true });
  }

  // Every change drawn in the main column may change the boxes; a narrower
  // column moves them.
  const watched = new WeakSet();
  let queued = 0;
  const sized = new ResizeObserver(entries => entries.forEach(e => slide(e.target, JUMP_PX)));
  function watch(main) {
    if (watched.has(main)) return;
    watched.add(main);
    new MutationObserver(records => {
      if (queued || records.every(r => r.target instanceof Element && r.target.closest(".stage-bento, .stage-dock"))) return;
      queued = requestAnimationFrame(() => { queued = 0; refresh(); });
    }).observe(main, { childList: true, subtree: true });
  }

  let drawing = null;
  /** Draws the bento under the conversation and the dock in its head. */
  function refresh() {
    const main = document.getElementById("main");
    if (!main) return Promise.resolve();
    watch(main);
    const conv = main.querySelector(":scope > section.conv");
    if (!on() || !conv) {
      main.querySelector(":scope > .stage-bento")?.remove();
      document.querySelectorAll(".stage-dock").forEach(d => d.remove());
      return Promise.resolve();
    }
    if (drawing) return drawing;
    drawing = (async () => {
      const list = await boxes();
      let bento = main.querySelector(":scope > .stage-bento");
      if (!bento) { bento = document.createElement("nav"); bento.className = "stage-bento"; bento.setAttribute("aria-label", "What is happening"); main.append(bento); sized.observe(bento); at = new WeakMap(); }
      place(bento, list);
      const row = conv.querySelector("#conv-head .row, .conv-head .row");
      if (row) {
        let d = row.querySelector(":scope > .stage-dock");
        if (!d) { d = document.createElement("span"); d.className = "stage-dock"; row.prepend(d); }
        const markup = dock();
        if (d._html !== markup) { d.innerHTML = markup; d._html = markup; }
      }
      follow();
    })().catch(() => { /* The bento waits for the next change. */ }).finally(() => { drawing = null; });
    return drawing;
  }

  /**
   * The column beside the conversation: the canvas for what is open.
   * @param {{ kind: string, title: string, head?: string|null, backBtn: string, body: string }} v
   */
  function shell(v) {
    return `<section class="stage-artifact" aria-label="${html(v.title)}">
      <div class="artifact-top">${v.backBtn}<div class="artifact-title">${v.head || `<h2>${html(v.title)}</h2>`}</div><button type="button" class="artifact-close" data-toggle-inspector>Close</button></div>
      <div class="artifact-content detail-body" data-panel="${html(v.kind)}">${v.body}</div>
    </section>`;
  }

  /** The canvas slides in when it first opens. @param {Element} el @param {boolean} changed */
  function settle(el, changed) {
    const canvas = el.querySelector(".stage-artifact");
    if (!canvas || !changed) return;
    if (lastArtifact !== "open") canvas.classList.add("stage-opening");
    lastArtifact = "open";
  }

  /** The canvas is gone: the next one slides in again. */
  function closed() { lastArtifact = null; }

  /**
   * A new block of an answer streams in word by word, as the mockup's does.
   * @param {Element} block
   */
  function stream(block) {
    if (!on() || !moving()) return;
    const walk = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
    const texts = [];
    for (let n = walk.nextNode(); n; n = walk.nextNode()) if (n.textContent.trim() && !n.parentElement?.closest("pre, code, svg")) texts.push(n);
    let i = 0;
    for (const n of texts) {
      const frag = document.createDocumentFragment();
      for (const p of n.textContent.split(/(\s+)/)) {
        if (!p) continue;
        if (/^\s+$/.test(p) || i >= WORDS_STREAMED) { frag.append(p); continue; }
        const w = document.createElement("span");
        w.className = "stream-word";
        w.style.animationDelay = `${i++ * WORD_STEP_MS}ms`;
        w.textContent = p;
        frag.append(w);
      }
      n.replaceWith(frag);
    }
  }

  // The first conversation drawn starts the watch, without waiting for the
  // app's next poll.
  const first = new MutationObserver(() => { if (document.querySelector("#main > section.conv")) { first.disconnect(); refresh(); } });
  first.observe(document.documentElement, { childList: true, subtree: true });

  // The stage follows the style.
  let wasOn = null;
  window.addEventListener("magnus-theme", () => {
    const now = on();
    if (wasOn !== null && now !== wasOn && typeof Panel !== "undefined") {
      if (now && Panel.what === "activity") closePanel(false);
      else if (Panel.what) drawPanel();
    }
    wasOn = now;
    refresh();
  });

  return { on, refresh, boxes, shell, settle, closed, stream };
})();
globalThis.AuroraStage = AuroraStage;
