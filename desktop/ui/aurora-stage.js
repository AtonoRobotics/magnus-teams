// The Aurora stage: in a style that has one (Theme.stage), the conversation
// and its cards work as the mockup's agent demo does
// (decisions/2026-11-20-aurora-stage.md).
// - The conversation is the main card. Under it is a row of cards, one for
//   each companion view (Live Workspace, Approvals, Up Next, Recent Work,
//   Rules, Files, Apps, the computer) with the panel's icons. The conversation drives them:
//   the Live Workspace card wears the beam while its work runs and says what
//   it is doing, turns amber when it waits on the person and mint with the
//   result when done; Approvals turns amber with what waits. The cards move
//   to put what needs the person first.
// - There is no panel beside the conversation. Anything opened (a task, a
//   file, a decision, the full view behind a card) is the artifact canvas: it
//   slides in on the right, and the conversation and its cards narrow and
//   move over. When work the person is watching finishes or needs them, its
//   result slides open on its own, as the mockup's does. Close slides it out.
// - A new block of an answer streams in word by word.
"use strict";

const AuroraStage = (() => {
  const FILES_FRESH_MS = 30_000;
  const MOVE_MS = 460;
  const WORD_STEP_MS = 34;
  const WORDS_STREAMED = 120;
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

  /**
   * One card under the conversation, drawn as the mockup's small bento box.
   * @param {{ key: string, label: string, icon: string, svg?: string, open: string, href: string, tone: string, title: string, desc: string, count?: number }} c
   */
  function tileHtml(c) {
    return `<a class="stage-tile${c.tone === "run" ? " aurora-executing" : ""}" data-tile="${c.key}" data-tone="${c.tone}" href="${html(c.href)}" data-open="${html(c.open)}">
      <span class="stage-tile-label"><span class="stage-tile-ic" aria-hidden="true">${c.svg || icon(c.icon)}</span><span class="stage-tile-name">${html(c.label)}</span>${c.count ? `<span class="stage-tile-badge">${c.count}</span>` : ""}<span class="mini-dot" aria-hidden="true"></span></span>
      <strong class="stage-tile-title">${html(c.title)}</strong>
      <span class="stage-tile-desc">${html(c.desc)}</span>
    </a>`;
  }

  /** The cards, most urgent first. */
  async function tiles() {
    const tasks = App.tasks || [];
    const here = tasks.filter(t => App.thread && t.thread === App.thread);
    const pool = here.length ? here : tasks;
    const active = pool.filter(t => !ended(t));
    const run = active.find(waits) || active[0];
    const last = pool.filter(ended).sort((a, b) => (b.ended_at || b.created_at || "").localeCompare(a.ended_at || a.created_at || ""))[0];
    const pending = needsMe();
    const connectors = (typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : App.connectors) || [];
    const connected = connectors.filter(c => stateOf(c) === "connected");
    const scheds = (App.schedules || []).filter(sc => !sc.ended && !sc.paused).sort((a, b) => String(a.next || "~").localeCompare(String(b.next || "~")));
    const list = [];
    const live = { key: "live", label: "Live Workspace", icon: "sparkles" };
    if (run) {
      const step = stepOf(run);
      list.push({ ...live, tone: waits(run) ? "needs" : "run", open: `task:${run.task_id}`, href: `#/task/${enc(run.task_id)}`, title: named(run), desc: waits(run) ? "Waiting on you to decide" : run.status_line || (step && step.goal) || "Working…" });
    } else if (last) {
      list.push({ ...live, tone: last.status === "completed" ? "done" : "idle", open: `task:${last.task_id}`, href: `#/task/${enc(last.task_id)}`, title: named(last), desc: firstLine(last.summary) || (last.status === "completed" ? "Done" : "Stopped") });
    } else {
      list.push({ ...live, tone: "idle", open: "tasks", href: "#/tasks", title: "Connected Apps", desc: connected.length ? `Standing by across ${plural(connected.length, "app")}.` : "Standing by." });
    }
    const approvals = { key: "approvals", label: "Approvals", icon: "need", href: "#/inbox" };
    list.push(pending.length
      ? { ...approvals, tone: "needs", count: pending.length, open: "needs", title: pending[0].summary || pending[0].ask?.sentence || "A decision", desc: pending.length > 1 ? `${pending.length - 1} more waiting on you` : "Nothing goes out until you approve." }
      : { ...approvals, tone: "idle", open: "needs", title: "Nothing waiting", desc: "Nothing goes out until you approve." });
    const next = scheds[0];
    list.push({ key: "next", label: "Up Next", icon: "calendar", tone: "idle", open: next ? `schedule:${next.id}` : "schedules", href: "#/schedules",
      title: next ? next.title : "No routines yet", desc: next ? `${next.next ? until(next.next) : rule(next)} · ${plural(scheds.length, "routine")}` : "Ask Magnus to do something on a schedule." });
    list.push({ key: "recent", label: "Recent Work", icon: "clock", tone: "idle", open: "tasks", href: "#/tasks",
      title: last ? named(last) : "No work yet", desc: last ? `${last.status === "completed" ? "Done" : "Stopped"} · ${when(last.ended_at || last.created_at)}` : "Finished work shows here." });
    list.push({ key: "rules", label: "Rules", icon: "shield", tone: "idle", open: "rules", href: "#/rules", title: "Guardrails", desc: "What Magnus may do without asking." });
    const fl = await recentFiles();
    list.push({ key: "files", label: "Files", icon: "file", tone: "idle", open: fl[0] ? `file:${fl[0].id}` : "files", href: "#/files",
      title: fl[0] ? fl[0].name : "No files yet", desc: fl[0] ? `${when(fl[0].at)} · ${plural(fl.length, "file")}` : "What Magnus makes lands here." });
    list.push({ key: "apps", label: "Apps", icon: "grid", tone: "idle", open: "apps", href: "#/apps",
      title: connected.length ? `${plural(connected.length, "app")} connected` : "No apps yet", desc: connected.length ? connected.slice(0, 4).map(c => c.title).join(", ") : "Connect Gmail, Drive and more." });
    const comp = (App.computers || []).find(x => x.owner?.person === App.me?.id);
    if (comp && typeof controlWords === "function") {
      const w = controlWords(comp, false);
      const svg = typeof UI !== "undefined" && UI.computer ? `<svg class="ic" viewBox="0 0 24 24">${UI.computer}</svg>` : "";
      list.push({ key: "computer", label: "Computer", icon: "globe", svg, tone: w.key === "working" ? "run" : w.key === "asks" ? "needs" : "idle", open: `computer:${comp.id}`, href: `#/computer/${enc(comp.id)}`, title: "Cloud Computer", desc: w.pill });
    }
    const rank = c => (c.tone === "needs" ? 0 : c.tone === "run" ? 1 : 2);
    return list.map((c, i) => [c, i]).sort((a, b) => rank(a[0]) - rank(b[0]) || a[1] - b[1]).map(([c]) => tileHtml(c)).join("");
  }

  // Where each card is, so a redraw can slide them from there.
  const where = row => new Map([...row.querySelectorAll(".stage-tile[data-tile]")].map(t => [t.dataset.tile, t.getBoundingClientRect()]));
  function slide(row, before) {
    if (document.documentElement.dataset.motion !== "full") return;
    row.querySelectorAll(".stage-tile[data-tile]").forEach(t => {
      const from = before.get(t.dataset.tile);
      if (!from) return;
      const to = t.getBoundingClientRect();
      const dx = from.left - to.left, dy = from.top - to.top;
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return;
      t.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: MOVE_MS, easing: MOVE_EASE });
    });
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

  // Every change drawn in the main column may change the cards.
  const watched = new WeakSet();
  let queued = 0;
  function watch(main) {
    if (watched.has(main)) return;
    watched.add(main);
    new MutationObserver(records => {
      if (queued || records.every(r => r.target instanceof Element && r.target.closest(".stage-tiles"))) return;
      queued = requestAnimationFrame(() => { queued = 0; refresh(); });
    }).observe(main, { childList: true, subtree: true });
  }

  let drawing = null;
  /** Draws the cards under the conversation, when the style has a stage. */
  function refresh() {
    const main = document.getElementById("main");
    if (!main) return Promise.resolve();
    watch(main);
    const old = main.querySelector(":scope > .stage-tiles");
    if (!on() || !main.querySelector(":scope > section.conv")) { if (old) old.remove(); return Promise.resolve(); }
    if (drawing) return drawing;
    drawing = (async () => {
      const markup = await tiles();
      let row = main.querySelector(":scope > .stage-tiles");
      if (!row) { row = document.createElement("nav"); row.className = "stage-tiles"; row.setAttribute("aria-label", "Workspace"); main.append(row); }
      if (row._html !== markup) { const before = where(row); row.innerHTML = markup; row._html = markup; slide(row, before); }
      follow();
    })().catch(() => { /* The cards wait for the next change. */ }).finally(() => { drawing = null; });
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
    if (!on() || document.documentElement.dataset.motion !== "full") return;
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

  return { on, refresh, tiles, shell, settle, closed, stream };
})();
globalThis.AuroraStage = AuroraStage;
