// The Aurora stage: in a style that has one (Theme.stage), the conversation
// and its cards work as the mockup's agent demo does
// (decisions/2026-11-20-aurora-stage.md).
// - One frame holds the conversation and the canvas
//   (decisions/2026-11-21-aurora-composition.md). The input stays at the
//   bottom; just above it is the bento: a box appears
//   while something is relevant and leaves when it is not, and the others
//   move to their new places. Live Workspace while work in this conversation
//   runs (with the beam and what it is doing), waits on the person (amber) or
//   has just finished (mint, with its result); Approvals while a decision
//   waits; Up Next while a routine is due soon; Files while a new file is
//   fresh; Apps while one needs attention; the computer while it works or
//   asks for the person; Recent Work while work elsewhere has just finished;
//   Messages while a teammate has written and the person has not looked.
// - A decision is made where it shows (decisions/2026-11-21-aurora-alerts.md):
//   one about this conversation opens the canvas beside it, with what it
//   would do and its answers; one from elsewhere is a box in the bento with
//   its answers in the box. Answered, it says so in place, then goes.
// - Nothing floats over the stage: a message that would be a pop-up is a
//   box here instead (`takes`). One that comes while the page is still being
//   drawn waits to see whether there is a stage to show it on.
// - Every view stays one click away in the conversation's head: a row of the
//   panel's icons (Approvals, Tasks, Routines, Rules, Files, Apps).
// - There is no panel beside the conversation. Anything opened is the
//   artifact canvas: it slides in on the right, and the conversation and its
//   boxes narrow and move over. When work the person is watching finishes or
//   needs them, its result slides open on its own. Close slides it out.
// - A new block of an answer streams in word by word.
// - Sizes move (decisions/2026-11-21-aurora-sizes.md): when boxes come or go
//   the bento grows or shrinks over time, and the conversation with it,
//   instead of jumping; a box that keeps its place rides with the bento, and
//   a conversation read to its end stays at its end.
"use strict";

const AuroraStage = (() => {
  const FILES_FRESH_MS = 30_000;
  const RECENT_MS = 60 * 60_000;
  const DUE_SOON_MS = 24 * 60 * 60_000;
  const MOVE_MS = 460;
  const LEAVE_MS = 320;
  const JUMP_PX = 24;
  /** How near its end a conversation counts as read to its end. */
  const END_PX = 24;
  const WORD_STEP_MS = 34;
  const WORDS_STREAMED = 120;
  const SHOWN_BOXES = 4;
  /** How long a message stays a box when the person does not open it. */
  const NOTE_FRESH_MS = 30 * 60_000;
  /** How long an answered box shows the answer before it goes. */
  const ANSWER_SHOWN_MS = 900;
  const MOVE_EASE = "cubic-bezier(0.16, 1, 0.3, 1)";
  /** The bento folding: slow while a leaving box fades, then quick. */
  const FOLD_EASE = "cubic-bezier(0.65, 0, 0.35, 1)";
  const html = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  let files = { at: 0, list: [], loading: null };
  let lastArtifact = null;
  /** @type {Map<string, string>|null} each task's state when last seen */
  let seen = null;
  /** Messages that would have been pop-ups, by key, newest last. @type {Map<string, { key: string, title: string, body: string, href: string, at: number }>} */
  const notes = new Map();
  /** Decisions of this conversation opened beside it once already. */
  const offered = new Set();
  /** What came to be told before the page was first drawn. @type {Array<{ key: string, title: string, body: string, href: string, system?: string }>} */
  const early = [];
  /** A box just answered, shown with its answer until `until`. @type {?{ box: any, until: number }} */
  let held = null;

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
    // The decision open beside the conversation is not also a box.
    const pending = needsMe().filter(i => `item:${i.id}` !== Panel.what && i.id !== held?.box.item);
    if (held && held.until > Date.now()) list.push(held.box);
    else held = null;
    if (pending.length) {
      const i = pending[0];
      list.push({ key: `approval:${i.id}`, item: i.id, label: "Approvals", icon: "need", href: "#/inbox", tone: "needs", count: pending.length > 1 ? pending.length : 0, open: `item:${i.id}`,
        title: i.ask?.sentence || i.summary || "A decision", desc: pending.length > 1 ? `${pending.length - 1} more waiting on you` : "Nothing goes out until you answer.", answers: answersOf(i) });
    }
    for (const [k, n] of notes) if (Date.now() - n.at > NOTE_FRESH_MS) notes.delete(k);
    const said = [...notes.values()];
    if (said.length) {
      const n = said[said.length - 1];
      list.push({ key: "messages", label: "Messages", icon: "chat", tone: "talk", open: "", note: n.key, href: n.href || "#", title: n.title, desc: `${n.body || ""}${said.length > 1 ? ` · ${said.length - 1} more` : ""}`, count: said.length > 1 ? said.length : 0 });
    }
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
    // A box answered keeps its place while it shows the answer.
    const rank = c => (c.tone === "needs" || c.said ? 0 : c.tone === "run" || c.tone === "talk" ? 1 : 2);
    return list.map((c, i) => [c, i]).sort((a, b) => rank(a[0]) - rank(b[0]) || a[1] - b[1]).map(([c]) => c).slice(0, SHOWN_BOXES);
  }

  /**
   * A decision's answers as its card has them (asks.js), when it is an ask
   * the person can answer in one tap; otherwise the box opens it.
   * @param {any} i an inbox item
   */
  function answersOf(i) {
    if (i.kind !== "approval" || !i.can_respond || typeof askCard !== "function") return "";
    const t = document.createElement("template");
    t.innerHTML = askCard(i, true);
    const row = t.content.querySelector(".ask-answers");
    return row && !row.querySelector("textarea, input") ? row.outerHTML : "";
  }

  /** A box's inside, as the mockup's small bento box. */
  function boxInner(c) {
    const title = c.answers ? `<a class="stage-tile-title" href="${html(c.href)}" data-open="${html(c.open)}">${html(c.title)}</a>` : `<strong class="stage-tile-title">${html(c.title)}</strong>`;
    const foot = c.said ? `<div class="aurora-confirmed-row"><span class="aurora-confirmed-pill">✓ ${html(c.said)}</span></div>` : c.answers || `<span class="stage-tile-desc">${html(c.desc)}</span>`;
    return `<span class="stage-tile-label"><span class="stage-tile-ic" aria-hidden="true">${c.svg || icon(c.icon)}</span><span class="stage-tile-name">${html(c.label)}</span>${c.count ? `<span class="stage-tile-badge">${c.count}</span>` : ""}<span class="mini-dot" aria-hidden="true"></span></span>
      ${title}
      ${foot}`;
  }

  // Where each box is in the bento, so a change can slide them from there.
  // Measured within the bento: when the bento itself moves (it grows, the
  // conversation shrinks), a box that keeps its place moves with it, once.
  /** @type {WeakMap<Element, { left: number, top: number }>} */
  let at = new WeakMap();
  const boxesIn = bento => [...bento.querySelectorAll(":scope > .stage-tile:not(.stage-tile-leaving)")];
  /** @param {Element} b @param {DOMRect} box */
  const within = (b, box) => { const r = b.getBoundingClientRect(); return { left: r.left - box.left, top: r.top - box.top }; };
  function remember(bento) { const box = bento.getBoundingClientRect(); boxesIn(bento).forEach(b => at.set(b, within(b, box))); }
  const moving = () => document.documentElement.dataset.motion === "full";
  function slide(bento, min) {
    if (!moving()) return remember(bento);
    const box = bento.getBoundingClientRect();
    for (const b of boxesIn(bento)) {
      const from = at.get(b), to = within(b, box);
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
    const was = sizeOf(bento);
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
      let b = bento.querySelector(`:scope > .stage-tile[data-tile="${CSS.escape(c.key)}"]:not(.stage-tile-leaving)`);
      // A box with answers in it is not a link: its title is.
      const tag = c.answers || c.said ? "DIV" : "A";
      if (b && b.tagName !== tag) { b.remove(); b = null; }
      if (!b) {
        b = document.createElement(tag);
        b.dataset.tile = c.key;
        if (moving()) { b.classList.add("stage-tile-enter"); b.addEventListener("animationend", () => b.classList.remove("stage-tile-enter"), { once: true }); }
      }
      b.classList.add("stage-tile");
      b.classList.toggle("aurora-executing", c.tone === "run");
      b.dataset.tone = c.tone;
      if (tag === "A") {
        if (c.open) b.dataset.open = c.open; else delete b.dataset.open;
        b.setAttribute("href", c.href);
      } else {
        b.setAttribute("role", "group");
        b.setAttribute("aria-label", c.label);
      }
      if (c.note) b.dataset.note = c.note; else delete b.dataset.note;
      const inner = boxInner(c);
      if (b._html !== inner) { b.innerHTML = inner; b._html = inner; }
      bento.append(b);
    }
    bento.querySelectorAll(":scope > .stage-tile-leaving").forEach(b => bento.append(b));
    bento.dataset.count = String(list.length);
    grow(bento, was);
    slide(bento, 1);
  }

  /** The bento's height and the room above it, as drawn now. @param {HTMLElement} bento */
  function sizeOf(bento) {
    return { height: bento.getBoundingClientRect().height, margin: parseFloat(getComputedStyle(bento).marginTop) || 0 };
  }

  /**
   * The bento grows or shrinks from the size it was drawn at to the size it
   * has now, and the conversation above it with it. Growing, it keeps pace
   * with the box rising in; shrinking, it waits for the leaving box to fade
   * before its room closes. A conversation read to its end is kept at its
   * end while it shrinks.
   * @param {HTMLElement} bento @param {{ height: number, margin: number }} was
   */
  function grow(bento, was) {
    bento.getAnimations().forEach(a => { if (a.id === "stage-grow") a.cancel(); });
    if (!moving()) return;
    const now = sizeOf(bento);
    if (Math.abs(now.height - was.height) < 1 && Math.abs(now.margin - was.margin) < 1) return;
    const a = bento.animate([{ height: `${was.height}px`, marginTop: `${was.margin}px` }, { height: `${now.height}px`, marginTop: `${now.margin}px` }], { duration: MOVE_MS, easing: now.height < was.height ? FOLD_EASE : MOVE_EASE });
    a.id = "stage-grow";
    const thread = bento.closest("section.conv")?.querySelector(":scope > .thread");
    if (!thread || thread.scrollHeight - thread.scrollTop - thread.clientHeight > END_PX) return;
    const keep = () => { thread.scrollTop = thread.scrollHeight; if (a.playState === "running") requestAnimationFrame(keep); };
    requestAnimationFrame(keep);
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
    if (App.thread && decide(tasks)) return;
    if (!was || !App.thread) return;
    const turned = tasks.find(t => t.thread === App.thread && was.get(t.task_id) === "running" && ["completed", "needs"].includes(now.get(t.task_id) || ""));
    if (!turned || (Panel.what && !Panel.autoOpened) || Panel.what === `task:${turned.task_id}`) return;
    openPanel(`task:${turned.task_id}`, { auto: true });
  }

  /**
   * A decision about this conversation, not yet offered, opens beside it
   * with its answers, unless the person opened something else themselves.
   * @param {any[]} tasks @returns {boolean} whether one opened
   */
  function decide(tasks) {
    if (Panel.what && !Panel.autoOpened) return false;
    if (String(Panel.what || "").startsWith("item:")) return false;
    const mine = new Set(tasks.filter(t => t.thread === App.thread && t.waiting_item).map(t => t.waiting_item));
    const i = needsMe().find(x => x.id && x.can_respond !== false && !offered.has(x.id) && (x.thread === App.thread || mine.has(x.id)));
    if (!i) return false;
    offered.add(i.id);
    openPanel(`item:${i.id}`, { auto: true });
    setTimeout(refresh, 0);
    return true;
  }

  /**
   * A message that would be a pop-up, taken as a box of the bento instead.
   * A decision needs no box of its own here: the stage shows it already.
   * Where no conversation is shown (Settings, say), there is no stage to
   * show it on, and it is a pop-up as in the other styles. Before the page
   * is first drawn it is held, and placed once it is (`look`).
   * @param {{ key: string, title: string, body: string, href: string }} s
   * @returns {boolean} whether the stage took it
   */
  function takes(s) {
    if (!on()) return false;
    const main = document.getElementById("main");
    if (!main?.querySelector(":scope > section.conv")) {
      if (!main || main.firstElementChild) return false;
      early.push(s);
      first.observe(document.documentElement, { childList: true, subtree: true });
      return true;
    }
    if (s.key.startsWith("inbox:")) { refresh(); return true; }
    notes.delete(s.key);
    notes.set(s.key, { key: s.key, title: s.title, body: s.body, href: s.href, at: Date.now() });
    refresh();
    return true;
  }

  // A message opened, or its place visited, is no longer new.
  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const box = t.closest?.(".stage-tile[data-note]");
    if (box instanceof HTMLElement) {
      const n = notes.get(box.dataset.note || "");
      notes.delete(box.dataset.note || "");
      if (n && !n.href) e.preventDefault();
      refresh();
    }
    // A box answered keeps its place and says the answer, then goes.
    const btn = t.closest?.(".stage-tile .ask-answers button[data-decision]");
    const tile = btn?.closest(".stage-tile");
    if (!btn || !(tile instanceof HTMLElement) || !on()) return;
    const decision = (/** @type {HTMLElement} */ (btn).dataset.decision || "").split(":")[0];
    if (decision === "ask-more") return;
    const said = typeof AuroraAlerts !== "undefined" ? AuroraAlerts.said(decision) : "Done";
    const id = /** @type {HTMLElement} */ (btn).dataset.item || "";
    held = { box: { key: tile.dataset.tile, item: id, label: "Approvals", icon: "need", href: "#/inbox", tone: "done", open: `item:${id}`, title: tile.querySelector(".stage-tile-title")?.textContent || "", desc: "", said }, until: Date.now() + ANSWER_SHOWN_MS };
    // Its answers become the answer now; the box goes once it has shown.
    const row = tile.querySelector(".ask-answers");
    if (row) row.outerHTML = `<div class="aurora-confirmed-row"><span class="aurora-confirmed-pill">✓ ${html(said)}</span></div>`;
    tile.dataset.tone = "done";
    setTimeout(refresh, ANSWER_SHOWN_MS + 20);
  });
  window.addEventListener("hashchange", () => {
    for (const [k, n] of notes) if (n.href && location.hash.startsWith(n.href.split("/t/")[0])) notes.delete(k);
    refresh();
  });

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
  /** The bento, kept when the conversation is drawn again. @type {?HTMLElement} */
  let kept = null;

  /**
   * The bento in its place: after the thread, just above the input.
   * @param {Element} conv
   */
  function bentoIn(conv) {
    let bento = /** @type {?HTMLElement} */ (conv.querySelector(":scope > .stage-bento"));
    if (!bento) {
      if (!kept) {
        kept = document.createElement("nav");
        kept.className = "stage-bento";
        kept.setAttribute("aria-label", "What is happening");
        sized.observe(kept);
        at = new WeakMap();
      }
      bento = kept;
    }
    const thread = conv.querySelector(":scope > .thread");
    const before = conv.querySelector(":scope > .composer-slot");
    if (thread) { if (thread.nextElementSibling !== bento) thread.after(bento); }
    else if (before) { if (before.previousElementSibling !== bento) before.before(bento); }
    else if (bento.parentElement !== conv) conv.append(bento);
    return bento;
  }

  /** Draws the bento above the conversation's input and the dock in its head. */
  function refresh() {
    const main = document.getElementById("main");
    if (!main) return Promise.resolve();
    watch(main);
    const conv = main.querySelector(":scope > section.conv");
    if (!on() || !conv) {
      document.querySelectorAll(".stage-bento").forEach(b => b.remove());
      document.querySelectorAll(".stage-dock").forEach(d => d.remove());
      return Promise.resolve();
    }
    if (drawing) return drawing;
    drawing = (async () => {
      const list = await boxes();
      const now = main.querySelector(":scope > section.conv");
      if (!now) return;
      const bento = bentoIn(now);
      place(bento, list);
      const row = now.querySelector("#conv-head .row, .conv-head .row");
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
    // What the canvas shows is not also a box.
    setTimeout(refresh, 0);
  }

  /** The canvas is gone: the next one slides in again. */
  function closed() { lastArtifact = null; setTimeout(refresh, 0); }

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
  // app's next poll; and once the page is drawn, what came before it is
  // placed: on the stage, or as a pop-up where there is none.
  let started = false;
  function look() {
    const main = document.getElementById("main");
    if (early.length && main?.firstElementChild) {
      for (const s of early.splice(0)) if (!takes(s) && typeof Notices !== "undefined") Notices.popup(s);
    }
    if (!started && main?.querySelector(":scope > section.conv")) { started = true; refresh(); }
    if (started && !early.length) first.disconnect();
  }
  const first = new MutationObserver(look);
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

  return { on, refresh, boxes, shell, settle, closed, stream, takes, notes, offered };
})();
globalThis.AuroraStage = AuroraStage;
