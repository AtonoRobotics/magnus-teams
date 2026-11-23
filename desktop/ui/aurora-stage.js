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
// - Boxes come and go (decisions/2026-11-21-aurora-boxes-come-and-go.md,
//   aurora-boxes.js): one that wants the person stays until it is dealt
//   with; any other shows when it appears or changes, then leaves. Each has
//   a Close, which puts it away until it changes.
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
  const WORD_ANIM_MS = 340;
  const WORD_UNWRAP_BUFFER_MS = 80;
  const WORDS_STREAMED = 120;
  const SHOWN_BOXES = 4;
  const MAX_APPROVAL_BOXES = 2;
  const MAX_CONCURRENT_TASK_BOXES = 2;
  const PREVIEW_ROWS_MAX = 6;
  const PREVIEW_BODY_MAX_CHARS = 600;
  const WIDE_CANVAS = new Set(["computer", "file", "task", "schedule", "files", "shared"]);
  const PREVIEW_HEADER_RE = /^(to|recipient|recipients|cc|bcc|channel|room|destination|url|host|site|file|path|calendar|event|subject|title|summary|amount|total|price|max_usd)$/i;
  const PREVIEW_BODY_RE = /^(body|message|text|content|note|description|html|draft)$/i;
  /** After a box's time is up, so it is up when it is looked at. */
  const LEAVE_SLACK_MS = 50;
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
  const pctOf = t => {
    const units = t?.plan?.units || [];
    if (!units.length) return null;
    return Math.round((100 * units.filter(u => u.status === "done").length) / units.length);
  };
  const named = t => t.title || firstLine(t.text) || "Task";
  const tsOf = iso => { const at = Date.parse(iso || ""); return Number.isNaN(at) ? 0 : at; };
  const age = iso => { const at = Date.parse(iso || ""); return Number.isNaN(at) ? Infinity : Date.now() - at; };
  const latest = list => list.sort((a, b) => (b.ended_at || b.created_at || "").localeCompare(a.ended_at || a.created_at || ""))[0];
  const ATTENTION = ["needs_attention", "needs_more_access", "connecting"];

  /** Concise destination/consequence preview for an approval bento tile. @param {any} i @param {number} pendingCount */
  function approvalDesc(i, pendingCount) {
    const fields = typeof askSummaryFields === "function" ? askSummaryFields(i) : { rows: [], body: "" };
    const bits = [
      ...fields.rows.slice(0, 2).map(([k, v]) => `${k}: ${v}`),
      i.consequence || fields.body || "",
    ].filter(Boolean);
    if (bits.length) return bits.join(" · ");
    return pendingCount > 1 ? `${pendingCount - 1} more waiting on you` : "Nothing goes out until you answer.";
  }

  /**
   * What is relevant now, dynamically ordered by urgency and recency: each a box of the bento.
   * @returns {Promise<Array<{ key: string, label: string, icon: string, svg?: string, open: string, href: string, tone: string, title: string, desc: string, count?: number, pct?: number|null, updatedAt?: number }>>}
   */
  async function boxes() {
    const tasks = App.tasks || [];
    const here = tasks.filter(t => App.thread && t.thread === App.thread);
    const activeAll = tasks.filter(t => !ended(t)).sort((a, b) => (b.created_at || "").localeCompare(a.created_at || ""));
    const run = here.filter(t => !ended(t)).find(waits) || here.find(t => !ended(t));
    const last = latest(here.filter(t => ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    const list = [];
    if (typeof App !== "undefined" && App.shared && typeof SharedCanvas !== "undefined") list.push(...SharedCanvas.boxes());
    const live = { key: "live", label: "Live Workspace", icon: "sparkles" };
    // A request that became several tasks says how far it has got.
    const tally = here.length > 1 ? ` · ${here.filter(t => t.status === "completed").length} of ${here.length} done` : "";
    if (run) {
      const step = stepOf(run);
      const subNote = run.sub_agents_active ? ` · ${plural(run.sub_agents_active, "sub-agent")}` : "";
      list.push({
        ...live,
        tone: waits(run) ? "needs" : "run",
        open: `task:${run.task_id}`,
        href: `#/task/${enc(run.task_id)}`,
        title: named(run),
        desc: `${waits(run) ? "Waiting on you to decide" : run.status_line || (step && step.goal) || "Working…"}${subNote}${tally}`,
        pct: pctOf(run),
        count: activeAll.length > 1 ? activeAll.length : 0,
        updatedAt: tsOf(run.updated_at || run.created_at),
      });
    } else if (last) {
      list.push({
        ...live,
        tone: last.status === "completed" ? "done" : "idle",
        open: `task:${last.task_id}`,
        href: `#/task/${enc(last.task_id)}`,
        title: named(last),
        desc: firstLine(last.summary) || (last.status === "completed" ? "Done" : "Stopped"),
        updatedAt: tsOf(last.ended_at || last.created_at),
      });
    }
    // Surface concurrent autonomous tasks running in parallel (this thread or others).
    const othersRunning = activeAll.filter(t => !run || t.task_id !== run.task_id).slice(0, MAX_CONCURRENT_TASK_BOXES);
    for (const other of othersRunning) {
      const step = stepOf(other);
      const subNote = other.sub_agents_active ? ` · ${plural(other.sub_agents_active, "sub-agent")}` : "";
      list.push({
        key: `task:${other.task_id}`,
        label: other.thread === App.thread ? "Parallel Task" : "Autonomous Task",
        icon: "sparkles",
        tone: waits(other) ? "needs" : "run",
        open: `task:${other.task_id}`,
        href: `#/task/${enc(other.task_id)}`,
        title: named(other),
        desc: `${waits(other) ? "Waiting on you to decide" : other.status_line || (step && step.goal) || "Running in background…"}${subNote}`,
        pct: pctOf(other),
        updatedAt: tsOf(other.updated_at || other.created_at),
      });
    }
    // Approvals already open beside the conversation (either the list or this decision) are not also a box.
    const pending = Panel.what === "needs" ? [] : needsMe().filter(i => `item:${i.id}` !== Panel.what && i.id !== held?.box.item);
    if (held && held.until > Date.now()) list.push(held.box);
    else held = null;
    // Show up to MAX_APPROVAL_BOXES unclosed pending decisions with rich previews & 1-tap actions.
    const ask = (i, idx) => ({
      key: `approval:${i.id}`,
      item: i.id,
      label: "Approvals",
      icon: "need",
      href: "#/inbox",
      tone: "needs",
      count: idx === 0 && pending.length > 1 ? pending.length : 0,
      open: `item:${i.id}`,
      title: i.ask?.sentence || i.summary || "A decision",
      desc: approvalDesc(i, pending.length),
      updatedAt: tsOf(i.created_at),
    });
    let shownApprovals = 0;
    for (const i of pending) {
      if (shownApprovals >= MAX_APPROVAL_BOXES) break;
      const box = ask(i, shownApprovals);
      if (AuroraBoxes.isClosed(box)) continue;
      list.push({ ...box, answers: answersOf(i) });
      shownApprovals++;
    }
    for (const [k, n] of notes) if (Date.now() - n.at > NOTE_FRESH_MS) notes.delete(k);
    const said = [...notes.values()];
    if (said.length) {
      const n = said[said.length - 1];
      list.push({ key: "messages", label: "Messages", icon: "chat", tone: "talk", open: "", note: n.key, href: n.href || "#", title: n.title, desc: `${n.body || ""}${said.length > 1 ? ` · ${said.length - 1} more` : ""}`, count: said.length > 1 ? said.length : 0, updatedAt: n.at });
    }
    const comp = (App.computers || []).find(x => x.owner?.person === App.me?.id);
    const w = comp && typeof controlWords === "function" ? controlWords(comp, false) : null;
    if (comp && w && ["working", "asks", "waking"].includes(w.key)) {
      const svg = typeof UI !== "undefined" && UI.computer ? `<svg class="ic" viewBox="0 0 24 24">${UI.computer}</svg>` : "";
      list.push({ key: "computer", label: "Computer", icon: "globe", svg, tone: w.key === "asks" ? "needs" : "run", open: `computer:${comp.id}`, href: `#/computer/${enc(comp.id)}`, title: "Cloud Computer", desc: w.pill, updatedAt: tsOf(comp.updated_at) });
    }
    const due = (App.schedules || []).filter(sc => !sc.ended && !sc.paused && sc.next && -age(sc.next) < DUE_SOON_MS).sort((a, b) => String(a.next).localeCompare(String(b.next)));
    if (due.length) list.push({ key: "next", label: "Up Next", icon: "calendar", tone: "idle", open: `schedule:${due[0].id}`, href: "#/schedules", title: due[0].title, desc: `${until(due[0].next)}${due.length > 1 ? ` · ${due.length - 1} more today` : ""}`, updatedAt: tsOf(due[0].next) });
    const fresh = (await recentFiles()).filter(f => age(f.at) < RECENT_MS);
    if (fresh.length) list.push({ key: "files", label: "Files", icon: "file", tone: "done", open: `file:${fresh[0].id}`, href: "#/files", title: fresh[0].name, desc: fresh.length > 1 ? `${plural(fresh.length, "new file")} · ${when(fresh[0].at)}` : `New · ${when(fresh[0].at)}`, updatedAt: tsOf(fresh[0].at) });
    const connectors = (typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : App.connectors) || [];
    const asking = connectors.filter(c => ATTENTION.includes(stateOf(c)));
    if (asking.length) list.push({ key: "apps", label: "Apps", icon: "grid", tone: "needs", open: `connector:${asking[0].name}`, href: "#/apps", title: asking[0].title, desc: asking.length > 1 ? `${asking.length} apps need attention` : "Needs attention to keep working", updatedAt: 0 });
    // What happened while the person was away, in place of the banner over
    // the thread; opened, it goes until the next session.
    const feed = App.feed;
    if (feed && !App.feedDismissed) {
      const said = [[feed.ended?.length, "finished"], [feed.arrived?.length, "new for you"], [feed.decided?.length, "decided"]].filter(([n]) => n).map(([n, w]) => `${n} ${w}`);
      if (said.length) list.push({ key: "away", label: "Since you were away", icon: "clock", tone: "idle", open: "activity", href: "#/activity", title: said.join(" · "), desc: "See what happened", updatedAt: 0 });
    }
    const elsewhere = latest(tasks.filter(t => t.thread !== App.thread && ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    if (elsewhere) list.push({ key: "recent", label: "Recent Work", icon: "clock", tone: elsewhere.status === "completed" ? "done" : "idle", open: `task:${elsewhere.task_id}`, href: "#/tasks", title: named(elsewhere), desc: firstLine(elsewhere.summary) || `${elsewhere.status === "completed" ? "Done" : "Stopped"} · ${when(elsewhere.ended_at || elsewhere.created_at)}`, updatedAt: tsOf(elsewhere.ended_at || elsewhere.created_at) });
    // Record live content updates first so dynamic recency sorting immediately promotes changed boxes.
    if (typeof AuroraBoxes.noteChanges === "function") AuroraBoxes.noteChanges(list, Date.now());
    const rank = c => (c.tone === "needs" || c.said ? 0 : c.tone === "run" ? 1 : c.tone === "talk" ? 2 : c.tone === "done" ? 3 : 4);
    const recency = c => Math.max(c.updatedAt || 0, typeof AuroraBoxes.touchedAt === "function" ? AuroraBoxes.touchedAt(c.key) : 0);
    return list.map((c, i) => [c, i]).sort((a, b) => rank(a[0]) - rank(b[0]) || recency(b[0]) - recency(a[0]) || a[1] - b[1]).map(([c]) => c);
  }

  /**
   * A decision's answers as its card has them (asks.js), when it is an ask
   * the person can answer in one tap; otherwise the box opens it.
   * @param {any} i an inbox item
   */
  /** @type {Map<string, { sig: string, html: string }>} */
  const answersCache = new Map();
  const ANSWERS_CACHE_MAX = 64;

  function answersOf(i) {
    if (i.kind !== "approval" || !i.can_respond || typeof askCard !== "function") return "";
    const sig = `${i.status || ""}|${i.updated_at || ""}|${JSON.stringify(i.ask || null)}`;
    const hit = answersCache.get(i.id);
    if (hit && hit.sig === sig) return hit.html;
    const t = document.createElement("template");
    t.innerHTML = askCard(i, true);
    const row = t.content.querySelector(".ask-answers");
    const out = row && !row.querySelector("textarea, input") ? row.outerHTML : "";
    if (answersCache.size >= ANSWERS_CACHE_MAX) answersCache.clear();
    answersCache.set(i.id, { sig, html: out });
    return out;
  }

  /** A box's inside, as the mockup's small bento box. */
  function boxInner(c) {
    const title = c.answers ? `<a class="stage-tile-title" href="${html(c.href)}" data-open="${html(c.open)}">${html(c.title)}</a>` : `<strong class="stage-tile-title">${html(c.title)}</strong>`;
    // A box just answered goes on its own in a moment: it needs no Close.
    const close = c.said ? "" : `<button type="button" class="stage-tile-close" aria-label="Close ${html(c.label)}" title="Close">${icon("close")}</button>`;
    const bar = typeof c.pct === "number" ? `<div class="stage-tile-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${c.pct}"><span style="width:${c.pct}%"></span></div>` : "";
    const descHtml = c.desc ? `<span class="stage-tile-desc">${html(c.desc)}</span>` : "";
    const foot = c.said
      ? `<div class="aurora-confirmed-row"><span class="aurora-confirmed-pill">✓ ${html(c.said)}</span></div>`
      : c.answers
        ? `${descHtml}${c.answers}`
        : `${bar}${descHtml}`;
    return `<span class="stage-tile-label"><span class="stage-tile-ic" aria-hidden="true">${c.svg || icon(c.icon)}</span><span class="stage-tile-name">${html(c.label)}</span>${c.count ? `<span class="stage-tile-badge">${c.count}</span>` : ""}<span class="mini-dot" aria-hidden="true"></span>${close}</span>
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
      const from = at.get(b);
      b.getAnimations?.().forEach(a => { if (a.id === "stage-slide") a.cancel(); });
      const to = within(b, box);
      at.set(b, to);
      if (!from || b.classList.contains("stage-tile-enter")) continue;
      const dx = from.left - to.left, dy = from.top - to.top;
      if (Math.abs(dx) < min && Math.abs(dy) < min) continue;
      const a = b.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: MOVE_MS, easing: MOVE_EASE });
      a.id = "stage-slide";
    }
  }

  /**
   * The bento shows exactly these boxes: new ones come in, gone ones leave
   * from where they were, and the rest slide to their new places without
   * unnecessary DOM node churn when order is unchanged.
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
    const desiredNodes = [];
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
      b.dataset.sign = AuroraBoxes.sign(c);
      const inner = boxInner(c);
      if (b._html !== inner) { b.innerHTML = inner; b._html = inner; }
      desiredNodes.push(b);
    }
    const currentNodes = boxesIn(bento);
    const sameOrder = currentNodes.length === desiredNodes.length && desiredNodes.every((n, idx) => currentNodes[idx] === n);
    if (!sameOrder) {
      for (const b of desiredNodes) bento.append(b);
      bento.querySelectorAll(":scope > .stage-tile-leaving").forEach(b => bento.append(b));
    }
    bento.dataset.count = String(list.length);
    // Measure FLIP slide destinations BEFORE starting container height animation.
    slide(bento, 1);
    grow(bento, was);
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

  /** The conversation head's slim dock: Approvals direct, other views in one popover (context-aware for shared projects). */
  function dock() {
    const n = needsMe().length;
    const more = (typeof App !== "undefined" && App.shared && typeof SharedCanvas !== "undefined")
      ? SharedCanvas.dockItems(App.shared.ws)
      : [
        ["tasks", "#/tasks", "clock", "Tasks"],
        ["schedules", "#/schedules", "calendar", "Routines"],
        ["rules", "#/rules", "shield", "Rules"],
        ["files", "#/files", "file", "Files"],
        ["apps", "#/apps", "grid", "Apps"],
      ];
    const items = more.map(([open, href, ic, label]) =>
      `<a class="stage-dock-item" role="menuitem" href="${href}" data-open="${open}">${icon(ic)}<span>${label}</span></a>`
    ).join("");
    return `<a class="icon-btn stage-dock-btn" href="#/inbox" data-open="needs" title="Approvals" aria-label="Approvals${n ? `, ${n} waiting` : ""}">${icon("need")}${n ? `<span class="stage-dock-badge" aria-hidden="true">${n}</span>` : ""}</a>`
      + `<details class="stage-dock-more"><summary class="icon-btn stage-dock-btn" aria-label="Workspace views" title="Workspace views">${icon("grid")}</summary><div class="stage-dock-menu" role="menu" aria-label="Workspace views">${items}</div></details>`;
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

  // A box closed is put away until it changes; nothing behind it opens.
  document.addEventListener("click", e => {
    const x = /** @type {HTMLElement} */ (e.target).closest?.(".stage-tile-close");
    const tile = x?.closest(".stage-tile");
    if (!(tile instanceof HTMLElement)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    AuroraBoxes.close(tile.dataset.sign || "");
    if (tile.dataset.tile === "away" && typeof App !== "undefined") { App.feedDismissed = true; if (typeof banners === "function") banners(); }
    if (tile.dataset.note) notes.delete(tile.dataset.note);
    refresh();
  }, true);

  // A message opened, or its place visited, is no longer new.
  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    // What happened while away, opened, is seen until the next session.
    if (t.closest?.('.stage-tile[data-tile="away"]') && typeof App !== "undefined") {
      App.feedDismissed = true;
      if (typeof banners === "function") banners();
      refresh();
    }
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
  // Dismiss the Views popover when an item is chosen, on outside click, or on Escape.
  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const chosen = t.closest?.(".stage-dock-item");
    if (chosen) {
      const m = chosen.closest(".stage-dock-more");
      if (m instanceof HTMLDetailsElement) m.open = false;
      return;
    }
    if (!t.closest?.(".stage-dock-more")) {
      document.querySelectorAll(".stage-dock-more[open]").forEach(m => { /** @type {HTMLDetailsElement} */ (m).open = false; });
    }
  });
  document.addEventListener("keydown", e => {
    if (e.key !== "Escape") return;
    const openMenu = document.querySelector(".stage-dock-more[open]");
    if (!(openMenu instanceof HTMLDetailsElement)) return;
    openMenu.open = false;
    /** @type {HTMLElement|null} */ (openMenu.querySelector(":scope > summary"))?.focus();
  });

  window.addEventListener("hashchange", () => {
    for (const [k, n] of notes) if (n.href && location.hash.startsWith(n.href.split("/t/")[0])) notes.delete(k);
    refresh();
  });

  // Honor manual #workspace-splitter drags/keys; double-click returns to content-adaptive sizing.
  document.addEventListener("pointerdown", e => {
    const sep = /** @type {HTMLElement} */ (e.target).closest?.("#workspace-splitter");
    if (!sep || e.button !== 0) return;
    const mark = () => { document.body.dataset.splitterCustom = "1"; };
    window.addEventListener("pointermove", mark, { once: true });
    window.addEventListener("pointerup", () => window.removeEventListener("pointermove", mark), { once: true });
  });
  document.addEventListener("keydown", e => {
    if (e.target instanceof Element && e.target.id === "workspace-splitter" && ["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
      document.body.dataset.splitterCustom = "1";
    }
  });
  document.addEventListener("dblclick", e => {
    if (/** @type {HTMLElement} */ (e.target).closest?.("#workspace-splitter")) delete document.body.dataset.splitterCustom;
  });

  // Every change drawn in the main column may change the boxes; a narrower
  // column moves them.
  const watched = new WeakSet();
  /** @type {WeakMap<Element, number>} */
  const lastBentoWidth = new WeakMap();
  let queued = 0;
  const sized = new ResizeObserver(entries => entries.forEach(e => {
    const w = Math.round(e.contentRect?.width ?? e.target.getBoundingClientRect().width);
    const prev = lastBentoWidth.get(e.target);
    lastBentoWidth.set(e.target, w);
    if (prev === undefined || Math.abs(w - prev) < 1) return;
    if (e.target.getAnimations?.().some(a => a.id === "stage-grow" && a.playState === "running")) return;
    slide(e.target, JUMP_PX);
  }));
  function watch(main) {
    if (watched.has(main)) return;
    watched.add(main);
    new MutationObserver(records => {
      if (queued || records.every(r => r.target instanceof Element && r.target.closest(".stage-bento, .stage-dock, .aurora-canvas-row"))) return;
      queued = requestAnimationFrame(() => { queued = 0; refresh(); });
    }).observe(main, { childList: true, subtree: true });
  }

  let drawing = null;
  let rerun = false;
  /** The next box to leave on its own. @type {ReturnType<typeof setTimeout>|undefined} */
  let leaving;
  /** The bento, kept when the conversation is drawn again. @type {?HTMLElement} */
  let kept = null;

  /**
   * Adds a quiet "Inspect Canvas →" footer row on completed answers backed by
   * a finished task or rich artifact so users can reopen work after its bento
   * tile auto-dismisses.
   * @param {Element} conv
   */
  function annotateAnswers(conv) {
    const tasks = typeof App !== "undefined" && Array.isArray(App.tasks) ? App.tasks : [];
    const byId = new Map(tasks.map(t => [t.task_id, t]));
    conv.querySelectorAll('.entry[data-kind="answer"] > .msg.magnus[data-task]').forEach(msg => {
      if (msg.querySelector(":scope > .aurora-canvas-row")) return;
      const id = /** @type {HTMLElement} */ (msg).dataset.task || "";
      if (!id) return;
      const t = byId.get(id);
      const rich = Boolean(msg.querySelector(".shown-frame, .previews, .fchip-inline"));
      if (!t && !rich) return;
      if (t && !ended(t) && !rich) return;
      const meta = t ? `${named(t)}${t.summary ? ` · ${firstLine(t.summary)}` : ""}` : "Work result";
      const verified = !!(t && (t.verified || t.receipts_count > 0 || t.has_deliverable || (Array.isArray(t.receipts) && t.receipts.length > 0)));
      const badge = verified ? `<span class="aurora-verified-pill">✓ Verified</span>` : "";
      const row = document.createElement("div");
      row.className = "aurora-canvas-row";
      row.innerHTML = `${badge}<span class="aurora-canvas-meta">${html(meta)}</span><button type="button" class="aurora-canvas-open" data-open="task:${html(id)}">Inspect Canvas →</button>`;
      msg.append(row);
    });
  }

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
        // Boxes the person was on leave SHOWN_FOR_MS after they go.
        const away = () => setTimeout(() => {
          if (!kept || kept.matches(":hover, :focus-within")) return;
          AuroraBoxes.left(boxesIn(kept).map(b => b.dataset.sign || ""));
          refresh();
        }, 0);
        kept.addEventListener("pointerleave", away);
        kept.addEventListener("focusout", away);
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
      document.querySelectorAll(".aurora-canvas-row").forEach(r => r.remove());
      return Promise.resolve();
    }
    if (drawing) { rerun = true; return drawing; }
    drawing = (async () => {
      const all = await boxes();
      const now = main.querySelector(":scope > section.conv");
      if (!now) return;
      const bento = bentoIn(now);
      const held = new Set(bento.matches(":hover, :focus-within") ? boxesIn(bento).map(b => b.dataset.sign || "") : []);
      const list = AuroraBoxes.shown(all, Date.now(), held).slice(0, SHOWN_BOXES);
      place(bento, list);
      // A box that leaves on its own leaves on time, with nothing else changing.
      clearTimeout(leaving);
      const wait = AuroraBoxes.next(list);
      if (wait !== null) leaving = setTimeout(refresh, wait + LEAVE_SLACK_MS);
      const row = now.querySelector("#conv-head .row, .conv-head .row");
      if (row) {
        let d = row.querySelector(":scope > .stage-dock");
        if (!d) { d = document.createElement("span"); d.className = "stage-dock"; row.prepend(d); }
        const markup = dock();
        if (d._html !== markup) {
          const wasOpen = Boolean(d.querySelector(".stage-dock-more")?.hasAttribute("open"));
          d.innerHTML = markup;
          d._html = markup;
          if (wasOpen) d.querySelector(".stage-dock-more")?.setAttribute("open", "");
        }
      }
      annotateAnswers(now);
      follow();
    })().catch(() => { /* The bento waits for the next change. */ }).finally(() => {
      drawing = null;
      if (rerun) { rerun = false; refresh(); }
    });
    return drawing;
  }

  /**
   * The column beside the conversation: the canvas for what is open.
   * @param {{ kind: string, title: string, head?: string|null, backBtn: string, tabsHtml?: string, body: string }} v
   */
  function shell(v) {
    const size = WIDE_CANVAS.has(v.kind) ? "wide" : "compact";
    const tabs = v.tabsHtml || "";
    const d = v.kind === "item" ? decision(v.body) : null;
    if (d) {
      return `<section class="stage-artifact stage-decision" data-canvas-size="compact" aria-label="${html(d.title)}">
      <div class="artifact-top"><div class="artifact-title"><p class="decision-kind">${html(d.kind)}</p><h2>${html(d.title)}</h2></div><button type="button" class="artifact-close" data-toggle-inspector>Close</button></div>
      ${tabs}
      <div class="artifact-content detail-body" data-panel="item">${d.body}</div>
    </section>`;
    }
    return `<section class="stage-artifact" data-canvas-size="${size}" aria-label="${html(v.title)}">
      <div class="artifact-top">${v.backBtn}<div class="artifact-title">${v.head || `<h2>${html(v.title)}</h2>`}</div><button type="button" class="artifact-close" data-toggle-inspector>Close</button></div>
      ${tabs}
      <div class="artifact-content detail-body" data-panel="${html(v.kind)}">${v.body}</div>
    </section>`;
  }

  /**
   * Extracts a clean read-only preview of human-facing fields (To, Subject,
   * Body, URL, Amount) from `.ask-content .what .field` without altering the
   * exact audit payload inside `<details>`.
   * @param {Element} more
   * @returns {HTMLElement|null}
   */
  function decisionPreview(more) {
    const rows = [];
    const seenKeys = new Set();
    let bodyText = "";
    for (const field of more.querySelectorAll(".what > .field")) {
      const label = field.firstElementChild?.textContent?.trim() || "";
      const valEl = field.children[1];
      const val = valEl?.textContent?.trim() || "";
      if (!label || !val || val === "none") continue;
      const lower = label.toLowerCase();
      if (PREVIEW_BODY_RE.test(lower)) {
        if (!bodyText) bodyText = val.length > PREVIEW_BODY_MAX_CHARS ? `${val.slice(0, PREVIEW_BODY_MAX_CHARS).trimEnd()}…` : val;
      } else if (PREVIEW_HEADER_RE.test(lower) && !seenKeys.has(lower) && rows.length < PREVIEW_ROWS_MAX) {
        seenKeys.add(lower);
        const nice = label.replace(/_/g, " ");
        rows.push([nice[0].toUpperCase() + nice.slice(1), val]);
      }
    }
    if (!rows.length && !bodyText) return null;
    const card = document.createElement("div");
    card.className = "decision-preview";
    card.setAttribute("role", "region");
    card.setAttribute("aria-label", "Action preview");
    const rowsHtml = rows.length
      ? `<div class="decision-preview-rows">${rows.map(([k, v]) => `<div class="decision-preview-row"><span class="decision-preview-key">${html(k)}</span><span class="decision-preview-val">${html(v)}</span></div>`).join("")}</div>`
      : "";
    const bodyHtml = bodyText ? `<p class="decision-preview-body">${html(bodyText)}</p>` : "";
    card.innerHTML = `${rowsHtml}${bodyHtml}`;
    return card;
  }

  /**
   * A decision on the canvas is built around the decision
   * (decisions/2026-11-21-aurora-canvas.md): its question is the title, what
   * it would mean is under it, a scannable draft preview surfaces key fields,
   * and its answers sit at the canvas's foot. Raw fields and metadata stay
   * intact behind the single disclosure.
   * @param {string} body the item's card (app.js card())
   * @returns {?{ kind: string, title: string, body: string }}
   */
  function decision(body) {
    const t = document.createElement("template");
    t.innerHTML = body;
    const card = t.content.querySelector("article.card.item");
    const ask = card?.querySelector(".ask[data-ask]");
    const sentence = ask?.querySelector(".ask-sentence");
    if (!card || !ask || !sentence || !ask.querySelector(".ask-answers")) return null;
    const head = card.querySelector(":scope > .spread");
    const kind = head?.querySelector(".kind")?.textContent?.trim() || "Decision";
    const title = sentence.textContent?.trim() || kind;
    // The question is the canvas's title; the card keeps it, unseen, for
    // what reads the card (aurora-alerts.js).
    /** @type {HTMLElement} */ (sentence).hidden = true;
    const about = document.createElement("div");
    about.className = "decision-about";
    for (const el of [...card.children]) if (el !== head && el !== ask) about.append(el);
    const meta = head?.querySelector(".small.faint");
    if (meta) { const p = document.createElement("p"); p.className = "small faint decision-meta"; p.innerHTML = meta.innerHTML; about.append(p); }
    head?.remove();
    // Avoid duplicating askCard's inline .ask-preview when the canvas builds its own .decision-preview.
    ask.querySelector(":scope > .ask-preview")?.remove();
    let more = ask.querySelector(":scope > details.ask-content");
    if (!more && about.childNodes.length) {
      more = document.createElement("details");
      more.className = "ask-content";
      const summary = document.createElement("summary");
      summary.textContent = "Details";
      more.append(summary);
      ask.querySelector(":scope > .ask-answers")?.before(more);
    }
    if (more) {
      const preview = decisionPreview(more);
      if (preview) {
        more.before(preview);
        const sum = more.querySelector(":scope > summary");
        if (sum && (sum.textContent === "See exactly what it would do" || sum.textContent === "Details")) sum.textContent = "See exact details";
      }
      if (about.childNodes.length) more.querySelector(":scope > summary")?.after(about);
    }
    return { kind, title, body: card.outerHTML };
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
   * A new block of an answer streams in word by word, continuing sequentially
   * from `startIndex`, then unwraps its temporary spans back to plain text.
   * @param {Element} block @param {number} [startIndex]
   * @returns {number} next word cursor
   */
  function stream(block, startIndex = 0) {
    if (!on() || !moving()) return startIndex;
    const walk = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
    const texts = [];
    for (let n = walk.nextNode(); n; n = walk.nextNode()) if (n.textContent.trim() && !n.parentElement?.closest("pre, code, svg")) texts.push(n);
    let i = startIndex;
    /** @type {HTMLSpanElement[]} */
    const wrapped = [];
    for (const n of texts) {
      const frag = document.createDocumentFragment();
      for (const p of n.textContent.split(/(\s+)/)) {
        if (!p) continue;
        if (/^\s+$/.test(p) || i >= WORDS_STREAMED) { frag.append(p); continue; }
        const w = document.createElement("span");
        w.className = "stream-word";
        w.style.animationDelay = `${i++ * WORD_STEP_MS}ms`;
        w.textContent = p;
        wrapped.push(w);
        frag.append(w);
      }
      n.replaceWith(frag);
    }
    if (wrapped.length) {
      const waitMs = (i - 1) * WORD_STEP_MS + WORD_ANIM_MS + WORD_UNWRAP_BUFFER_MS;
      setTimeout(() => {
        for (const w of wrapped) if (w.isConnected) w.replaceWith(document.createTextNode(w.textContent || ""));
        if (block.isConnected) block.normalize();
      }, waitMs);
    }
    return i;
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
