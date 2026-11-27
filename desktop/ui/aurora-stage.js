// The Aurora stage: in a style that has one (Theme.stage), the conversation
// and its bento cards compose in one frame (decisions/2026-11-20-aurora-stage.md,
// decisions/2026-11-21-aurora-composition.md). Canvas shell, decision preview,
// history compaction, and word streaming live in aurora-stage-view.js.
"use strict";

const AuroraStage = (() => {
  const FILES_FRESH_MS = 30_000;
  const RECENT_MS = 60 * 60_000;
  const MOVE_MS = 460;
  const LEAVE_MS = 320;
  const JUMP_PX = 24;
  /** How near its end a conversation counts as read to its end. */
  const END_PX = 24;
  const SHOWN_BOXES = 3;
  const MAX_APPROVAL_BOXES = 1;
  const MAX_CONCURRENT_TASK_BOXES = 1;
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
  /** Messages that would have been pop-ups, by key, newest last. @type {Map<string, { key: string, title: string, body: string, href: string, at: number }>} */
  const notes = new Map();
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
  const stepOf = t => ((t.plan && t.plan.units) || []).find(u => u.status === "active");
  const pctOf = t => {
    const units = t?.plan?.units || [];
    if (!units.length) return null;
    return Math.round((100 * units.filter(u => u.status === "done").length) / units.length);
  };
  const planUnitsOf = t => ((t?.plan && Array.isArray(t.plan.units)) ? t.plan.units : []).slice(0, 5).map(u => ({
    goal: u.goal || "",
    status: u.status || "pending",
  }));
  const named = t => t.title || firstLine(t.text) || "Task";
  const tsOf = iso => { const at = Date.parse(iso || ""); return Number.isNaN(at) ? 0 : at; };
  const age = iso => { const at = Date.parse(iso || ""); return Number.isNaN(at) ? Infinity : Date.now() - at; };
  const latest = list => list.sort((a, b) => (b.ended_at || b.created_at || "").localeCompare(a.ended_at || a.created_at || ""))[0];
  const ATTENTION = ["needs_attention", "needs_more_access", "connecting"];
  /** What a finished task can show for itself: its actions and the receipts the server checked. @param {any} t */
  const proofOf = t => [t.actions ? plural(t.actions, "action") : "", t.receipts_count ? `${plural(t.receipts_count, "receipt")} checked` : ""].filter(Boolean).join(" · ");
  const STEP_MARK = /** @type {Record<string, string>} */ ({ done: "✓", failed: "✕", running: "…", stopped: "–" });
  /** A card's last actions, newest last, each with how it went. @param {Array<{ doing: string, state: string }>|undefined} steps */
  const stepsList = steps => (steps && steps.length ? `<ol class="stage-tile-steps">${steps.map(x => `<li data-state="${html(x.state)}"><span aria-hidden="true">${STEP_MARK[x.state] || ""}</span> ${html(x.doing)}</li>`).join("")}</ol>` : "");

  /** Read live and recent sub-agent rows rendered in the active thread so each gets a bento tile. */
  function threadSubAgents() {
    const out = [];
    const groups = [...document.querySelectorAll("#thread [data-sub-agents]")];
    const scope = groups.length ? groups[groups.length - 1] : document;
    for (const el of scope.querySelectorAll(".sub-agent")) {
      const run = el.getAttribute("data-sub-agent") || "";
      const status = el.getAttribute("data-status") || "";
      const goal = el.querySelector(".ttl")?.textContent?.trim() || "";
      const sub = el.querySelector(".sub")?.textContent?.trim() || "";
      const open = el.querySelector("a[data-open]")?.getAttribute("data-open") || "";
      const summary = el.querySelector(".sub-agent-result")?.textContent?.trim().replace(/\s+/g, " ").slice(0, 140) || "";
      if (goal) out.push({ run: run || goal, goal, status, sub, open, summary });
    }
    const rank = s => (s.status === "suspended" ? 0 : s.status === "running" ? 1 : s.status === "completed" ? 2 : 3);
    return out.sort((a, b) => rank(a) - rank(b)).slice(0, 4);
  }

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
   * What is relevant now, dynamically ordered by urgency and recency: each a box of the composed autonomous stage.
   * @returns {Promise<Array<any>>}
   */
  async function boxes() {
    // A schedule's routine run or quiet system task (e.g. Heartbeat that
    // found nothing) is no news: never show it on the stage unless it needs you.
    const tasks = (App.tasks || []).filter(t => !t.routine && !(t.system && !waits(t) && t.status !== "failed"));
    const here = tasks.filter(t => App.thread && t.thread === App.thread);
    const activeAll = tasks.filter(t => !ended(t)).sort((a, b) => (b.created_at || "").localeCompare(a.created_at || ""));
    const run = here.filter(t => !ended(t)).find(waits) || here.find(t => !ended(t));
    const last = latest(here.filter(t => ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    const threadSubs = threadSubAgents();
    const activeSubs = threadSubs.filter(s => s.status === "running" || s.status === "suspended");
    const list = [];
    if (typeof App !== "undefined" && App.shared && typeof SharedCanvas !== "undefined") list.push(...SharedCanvas.boxes());
    const live = { key: "live", label: "Live Workspace", icon: "sparkles" };
    // A request that became several tasks says how far it has got.
    const tally = here.length > 1 ? ` · ${here.filter(t => t.status === "completed").length} of ${here.length} done` : "";
    if (run) {
      const step = stepOf(run);
      const subNote = (run.sub_agents_active || activeSubs.length) ? ` · ${plural(run.sub_agents_active || activeSubs.length, "sub-agent")}` : "";
      list.push({
        ...live,
        tone: waits(run) ? "needs" : "run",
        taskId: run.task_id,
        open: `task:${run.task_id}`,
        href: `#/task/${enc(run.task_id)}`,
        title: named(run),
        desc: `${waits(run) ? "Waiting on you to decide" : run.status_line || (step && step.goal) || "Working…"}${subNote}${tally}`,
        steps: run.recent_steps,
        pct: pctOf(run),
        planSteps: planUnitsOf(run),
        subAgents: threadSubs.slice(0, 6),
        canStop: true,
        count: activeAll.length > 1 ? activeAll.length : 0,
        updatedAt: tsOf(run.updated_at || run.created_at),
      });
    } else if (activeSubs.length) {
      list.push({
        ...live,
        tone: activeSubs.some(s => s.status === "suspended") ? "needs" : "run",
        open: activeSubs[0].open || "tasks",
        href: "#/tasks",
        title: activeSubs.length === 1 ? activeSubs[0].goal : `${plural(activeSubs.length, "sub-agent")} working`,
        desc: activeSubs[0].sub || "Working on your request…",
        subAgents: activeSubs.slice(0, 6),
        count: activeSubs.length > 1 ? activeSubs.length : 0,
        updatedAt: Date.now(),
      });
    } else if (last) {
      list.push({
        ...live,
        tone: last.status === "completed" ? "done" : "idle",
        open: `task:${last.task_id}`,
        href: `#/task/${enc(last.task_id)}`,
        title: named(last),
        desc: firstLine(last.summary) || (last.status === "completed" ? "Done" : "Stopped"),
        steps: last.recent_steps,
        subAgents: threadSubs.slice(0, 6),
        proof: proofOf(last),
        verified: !!(last.verified || last.receipts_count > 0 || last.has_deliverable),
        updatedAt: tsOf(last.ended_at || last.created_at),
      });
    }
    // Surface at most one concurrent background task from another thread.
    const othersRunning = activeAll.filter(t => !run || t.task_id !== run.task_id).slice(0, MAX_CONCURRENT_TASK_BOXES);
    for (const other of othersRunning) {
      const step = stepOf(other);
      const subNote = other.sub_agents_active ? ` · ${plural(other.sub_agents_active, "sub-agent")}` : "";
      list.push({
        key: `task:${other.task_id}`,
        label: other.thread === App.thread ? "Parallel Task" : "Autonomous Task",
        icon: "sparkles",
        tone: waits(other) ? "needs" : "run",
        taskId: other.task_id,
        open: `task:${other.task_id}`,
        href: `#/task/${enc(other.task_id)}`,
        title: named(other),
        desc: `${waits(other) ? "Waiting on you to decide" : other.status_line || (step && step.goal) || "Running in background…"}${subNote}`,
        steps: other.recent_steps,
        pct: pctOf(other),
        planSteps: planUnitsOf(other),
        canStop: true,
        updatedAt: tsOf(other.updated_at || other.created_at),
      });
    }
    // Approvals already open in the sidebar are not duplicated on the stage.
    const pending = Panel.what === "needs" ? [] : needsMe().filter(i => `item:${i.id}` !== Panel.what && i.id !== held?.box.item);
    if (held && held.until > Date.now()) list.push(held.box);
    else held = null;
    // Show at most MAX_APPROVAL_BOXES unclosed pending decision with full structured preview & 1-tap actions.
    const ask = (i, idx) => {
      const fields = typeof askSummaryFields === "function" ? askSummaryFields(i) : { rows: [], body: "" };
      return {
        key: `approval:${i.id}`,
        item: i.id,
        label: "Needs your decision",
        icon: "need",
        href: "#/inbox",
        tone: "needs",
        tier: i.tier || "",
        tierHtml: typeof askTierBadge === "function" ? askTierBadge(i) : "",
        previewRows: fields.rows.slice(0, 4),
        previewBody: fields.body || "",
        consequence: i.consequence || "",
        count: idx === 0 && pending.length > 1 ? pending.length : 0,
        open: `item:${i.id}`,
        title: i.ask?.sentence || i.summary || "A decision",
        desc: approvalDesc(i, pending.length),
        updatedAt: tsOf(i.created_at),
      };
    };
    const batchIds = typeof batchableApprovals === "function" ? batchableApprovals(pending) : [];
    let shownApprovals = 0;
    for (const i of pending) {
      if (shownApprovals >= MAX_APPROVAL_BOXES) break;
      const box = ask(i, shownApprovals);
      if (AuroraBoxes.isClosed(box)) continue;
      list.push({ ...box, answers: answersOf(i), batchIds: shownApprovals === 0 && batchIds.length >= 2 ? batchIds : [] });
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
    // What is scheduled next is in Schedules: a run hours away is not something to look at now.
    const fresh = (await recentFiles()).filter(f => age(f.at) < RECENT_MS);
    if (fresh.length) list.push({ key: "files", label: "Files", icon: "file", tone: "done", open: `file:${fresh[0].id}`, href: "#/files", title: fresh[0].name, desc: fresh.length > 1 ? `${plural(fresh.length, "new file")} · ${when(fresh[0].at)}` : `New · ${when(fresh[0].at)}`, updatedAt: tsOf(fresh[0].at) });
    const connectors = (typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : App.connectors) || [];
    const asking = connectors.filter(c => ATTENTION.includes(stateOf(c)));
    if (asking.length) list.push({ key: "apps", label: "Apps", icon: "grid", tone: "needs", open: `connector:${asking[0].name}`, href: "#/apps", title: asking[0].title, desc: asking.length > 1 ? `${asking.length} apps need attention` : "Needs attention to keep working", updatedAt: 0 });
    const feed = App.feed;
    if (feed && !App.feedDismissed) {
      const said = [[feed.ended?.length, "finished"], [feed.arrived?.length, "new for you"], [feed.decided?.length, "decided"]].filter(([n]) => n).map(([n, w]) => `${n} ${w}`);
      if (said.length) list.push({ key: "away", label: "Since you were away", icon: "clock", tone: "idle", open: "activity", href: "#/activity", title: said.join(" · "), desc: "See what happened", updatedAt: 0 });
    }
    const elsewhere = latest(tasks.filter(t => t.thread !== App.thread && ended(t) && age(t.ended_at || t.created_at) < RECENT_MS));
    if (elsewhere) list.push({ key: "recent", label: "Recent Work", icon: "clock", tone: elsewhere.status === "completed" ? "done" : "idle", open: `task:${elsewhere.task_id}`, href: "#/tasks", title: named(elsewhere), desc: firstLine(elsewhere.summary) || `${elsewhere.status === "completed" ? "Done" : "Stopped"} · ${when(elsewhere.ended_at || elsewhere.created_at)}`, steps: elsewhere.recent_steps, proof: proofOf(elsewhere), verified: !!(elsewhere.verified || elsewhere.receipts_count > 0), updatedAt: tsOf(elsewhere.ended_at || elsewhere.created_at) });
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

  const SUB_STATE_LABEL = /** @type {Record<string, string>} */ ({ running: "Working", suspended: "Waiting", completed: "Done", failed: "Stopped", cancelled: "Stopped" });

  /** Whether a stage box contains inline expandable items or controls (so clicking it never yanks open the right sidebar). */
  function isInlineCard(c) {
    return !!(c.answers || c.said || c.canStop || c.key === "live" || (Array.isArray(c.subAgents) && c.subAgents.length) || (Array.isArray(c.planSteps) && c.planSteps.length) || (Array.isArray(c.steps) && c.steps.length));
  }

  /** A box's inside: single calm card with clickable inline expandable line items instead of spawning many cards or opening the sidebar on every click. */
  function boxInner(c) {
    const inline = isInlineCard(c);
    const title = `<strong class="stage-tile-title">${html(c.title)}</strong>`;
    const close = c.said ? "" : `<button type="button" class="stage-tile-close" aria-label="Close ${html(c.label)}" title="Close">${icon("close")}</button>`;
    const verifiedPill = c.verified ? `<span class="aurora-verified-pill">✓ Verified</span>` : "";
    const bar = typeof c.pct === "number" ? `<div class="stage-tile-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${c.pct}"><span style="width:${c.pct}%"></span></div>` : "";
    const planStepsHtml = Array.isArray(c.planSteps) && c.planSteps.length
      ? `<div class="stage-items">${c.planSteps.map((u, idx) => `<details class="stage-item" data-key="plan:${idx}" data-status="${html(u.status)}"><summary class="stage-item-row"><span class="stage-step-dot" aria-hidden="true"></span><span class="stage-item-title">${html(u.goal)}</span><span class="stage-item-meta">${html(u.status === "done" ? "Done" : u.status === "active" ? "In progress" : "Queued")}</span></summary><div class="stage-item-body"><p>${html(u.goal)}</p></div></details>`).join("")}</div>`
      : "";
    const subsHtml = Array.isArray(c.subAgents) && c.subAgents.length
      ? `<div class="stage-items">${c.subAgents.map(s => {
        const meta = SUB_STATE_LABEL[s.status] || s.status || "Working";
        const detail = s.summary || s.sub || (s.status === "completed" ? "Finished." : "Working in background…");
        const taskId = s.open?.startsWith("task:") ? s.open.slice(5) : "";
        const link = s.open ? `<a class="stage-item-link" href="${taskId ? `#/task/${enc(taskId)}` : "#/tasks"}" data-open="${html(s.open)}">Open full log →</a>` : "";
        return `<details class="stage-item" data-key="sub:${html(s.run)}" data-status="${html(s.status)}"><summary class="stage-item-row"><span class="stage-step-dot" aria-hidden="true"></span><span class="stage-item-title">${html(s.goal)}</span><span class="stage-item-meta">${html(meta)}</span></summary><div class="stage-item-body"><p>${html(detail)}</p>${link}</div></details>`;
      }).join("")}</div>`
      : "";
    const previewHtml = (Array.isArray(c.previewRows) && c.previewRows.length) || c.previewBody
      ? `<div class="stage-tile-preview">${c.previewRows?.length ? c.previewRows.map(([k, v]) => `<div class="stage-preview-row"><span class="stage-preview-key">${html(k)}</span><span class="stage-preview-val">${html(v)}</span></div>`).join("") : ""}${c.previewBody ? `<p class="stage-preview-body">${html(c.previewBody)}</p>` : ""}${c.consequence ? `<p class="stage-preview-note">${html(c.consequence)}</p>` : ""}</div>`
      : "";
    const stepsHtml = Array.isArray(c.steps) && c.steps.length
      ? `<details class="stage-item" data-key="steps:${html(c.key)}"><summary class="stage-item-row"><span class="stage-step-dot" aria-hidden="true"></span><span class="stage-item-title">${html(c.steps[c.steps.length - 1].doing)}</span><span class="stage-item-meta">${html(plural(c.steps.length, "step"))}</span></summary><div class="stage-item-body">${stepsList(c.steps)}</div></details>`
      : "";
    const descHtml = `${c.desc && !previewHtml ? `<span class="stage-tile-desc">${html(c.desc)}</span>` : ""}${stepsHtml}${c.proof ? `<span class="stage-tile-proof">${html(c.proof)}</span>` : ""}`;
    const actsHtml = c.canStop && c.taskId
      ? `<div class="stage-tile-acts"><a class="stage-tile-link" href="${html(c.href)}" data-open="${html(c.open)}">Full details →</a><button type="button" class="stage-tile-stop" data-stage-stop="${html(c.taskId)}">Stop</button></div>`
      : c.answers
        ? `<div class="stage-tile-acts-row">${c.answers}<a class="stage-tile-link" href="${html(c.href)}" data-open="${html(c.open)}">Full details →</a></div>`
        : (inline && c.open)
          ? `<div class="stage-tile-acts"><a class="stage-tile-link" href="${html(c.href)}" data-open="${html(c.open)}">Full details →</a></div>`
          : "";
    const batchHtml = Array.isArray(c.batchIds) && c.batchIds.length >= 2
      ? `<div class="stage-batch-bar"><span class="small muted">${c.batchIds.length} similar approvals waiting</span><button type="button" class="primary small" data-stage-batch-approve="${html(c.batchIds.join(","))}">Allow all ${c.batchIds.length}</button></div>`
      : "";
    const foot = c.said
      ? `<div class="aurora-confirmed-row"><span class="aurora-confirmed-pill">✓ ${html(c.said)}</span></div>`
      : `${c.tierHtml || ""}${bar}${descHtml}${previewHtml}${planStepsHtml}${subsHtml}${batchHtml}${actsHtml}`;
    return `<span class="stage-tile-label"><span class="stage-tile-ic" aria-hidden="true">${c.svg || icon(c.icon)}</span><span class="stage-tile-name">${html(c.label)}</span>${verifiedPill}${c.count ? `<span class="stage-tile-badge">${c.count}</span>` : ""}<span class="mini-dot" aria-hidden="true"></span>${close}</span>
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
      // Cards with expandable line items or controls are containers, not whole-card links.
      const tag = isInlineCard(c) ? "DIV" : "A";
      if (b && b.tagName !== tag) { b.remove(); b = null; }
      if (!b) {
        b = document.createElement(tag);
        b.dataset.tile = c.key;
        if (moving()) { b.classList.add("stage-tile-enter"); b.addEventListener("animationend", () => b.classList.remove("stage-tile-enter"), { once: true }); }
      }
      b.classList.add("stage-tile");
      b.classList.toggle("aurora-executing", c.tone === "run");
      b.dataset.tone = c.tone;
      if (c.tier) b.dataset.tier = c.tier; else delete b.dataset.tier;
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
      if (b._html !== inner) {
        const openKeys = new Set([...b.querySelectorAll("details[data-key][open]")].map(d => d.dataset.key));
        b.innerHTML = inner;
        b._html = inner;
        if (openKeys.size) b.querySelectorAll("details[data-key]").forEach(d => { if (openKeys.has(d.dataset.key)) d.open = true; });
      }
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
   * has now, and the conversation above it with it.
   * @param {HTMLElement} bento @param {{ height: number, margin: number }} was
   */
  function grow(bento, was) {
    bento.getAnimations().forEach(a => { if (a.id === "stage-grow") a.cancel(); });
    if (!moving()) return;
    const conv = bento.closest("section.conv");
    if (conv && bento.classList.contains("stage-bento") && conv.dataset.stageMode === "composed" && conv.getBoundingClientRect().width >= 820) return;
    const now = sizeOf(bento);
    if (Math.abs(now.height - was.height) < 1 && Math.abs(now.margin - was.margin) < 1) return;
    const a = bento.animate([{ height: `${was.height}px`, marginTop: `${was.margin}px` }, { height: `${now.height}px`, marginTop: `${now.margin}px` }], { duration: MOVE_MS, easing: now.height < was.height ? FOLD_EASE : MOVE_EASE });
    a.id = "stage-grow";
    const thread = conv?.querySelector(":scope > .thread");
    if (!thread || thread.scrollHeight - thread.scrollTop - thread.clientHeight > END_PX) return;
    const keep = () => { thread.scrollTop = thread.scrollHeight; if (a.playState === "running") requestAnimationFrame(keep); };
    requestAnimationFrame(keep);
  }

  /**
   * A message that would be a pop-up, taken as a box of the composed stage instead.
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

  // Inline Stop action directly on an active process tile in the composed stage.
  document.addEventListener("click", async e => {
    const stopBtn = /** @type {HTMLElement} */ (e.target).closest?.("[data-stage-stop]");
    if (!stopBtn || typeof Server === "undefined") return;
    e.preventDefault();
    const id = stopBtn.dataset.stageStop || "";
    if (!id) return;
    if (typeof ask === "function" && !await ask("Stop this task? What it already did stays done.", "Stop")) return;
    /** @type {HTMLButtonElement} */ (stopBtn).disabled = true;
    try {
      await Server.tasks.cancel(id);
      if (typeof toast === "function") toast("Stopped.");
      if (typeof globalThis.refresh === "function") await globalThis.refresh();
      refresh();
    } catch (err) {
      /** @type {HTMLButtonElement} */ (stopBtn).disabled = false;
      if (typeof toast === "function") toast(err.message);
    }
  });

  // 1-click batch approval on the Side Bento when 2+ approvals of one kind
  // wait, none of them money or irreversible (batchableApprovals, app.js).
  document.addEventListener("click", async e => {
    const btn = /** @type {?HTMLButtonElement} */ (/** @type {HTMLElement} */ (e.target).closest?.("[data-stage-batch-approve]"));
    if (!btn || typeof Server === "undefined" || !Server.inbox?.batch) return;
    e.preventDefault();
    const ids = (btn.dataset.stageBatchApprove || "").split(",").filter(Boolean);
    if (!ids.length) return;
    btn.disabled = true;
    try {
      await Server.inbox.batch(ids, { decision: "approve", scope: "once" });
      if (typeof toast === "function") toast(`Allowed ${ids.length} approvals.`);
      if (typeof globalThis.refresh === "function") await globalThis.refresh();
      refresh();
    } catch (err) {
      btn.disabled = false;
      if (typeof toast === "function") toast(err.message);
    }
  });

  // A message opened, or its place visited, is no longer new.
  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
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
  });
  // A box answered keeps its place and says the answer, then goes: once the
  // server took it (app.js tells "magnus:answered"), never at the tap.
  document.addEventListener("magnus:answered", e => {
    const { item, decision, button } = /** @type {CustomEvent<{ item: string, decision: string, button?: HTMLElement }>} */ (e).detail || {};
    const from = button?.closest?.(".stage-tile");
    if (!item || !decision || decision === "ask-more" || !(from instanceof HTMLElement) || !on()) return;
    const tile = from.isConnected ? from : document.querySelector(`.stage-tile[data-tile="${CSS.escape(from.dataset.tile || "")}"]:not(.stage-tile-leaving)`);
    const said = typeof AuroraAlerts !== "undefined" ? AuroraAlerts.said(decision) : "Done";
    held = { box: { key: from.dataset.tile, item, label: "Needs your decision", icon: "need", href: "#/inbox", tone: "done", open: `item:${item}`, title: from.querySelector(".stage-tile-title")?.textContent || "", desc: "", said }, until: Date.now() + ANSWER_SHOWN_MS };
    const row = tile?.querySelector(".stage-tile-acts-row, .ask-answers");
    if (row) row.outerHTML = `<div class="aurora-confirmed-row"><span class="aurora-confirmed-pill">✓ ${html(said)}</span></div>`;
    if (tile instanceof HTMLElement) tile.dataset.tone = "done";
    setTimeout(refresh, ANSWER_SHOWN_MS + 20);
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
    const onRecords = records => {
      if (queued || records.every(r => r.target instanceof Element && r.target.closest(".stage-bento, .stage-bento-bottom, .aurora-canvas-row"))) return;
      queued = requestAnimationFrame(() => { queued = 0; refresh(); });
    };
    if (typeof AuroraCore !== "undefined" && typeof AuroraCore.onDom === "function") {
      AuroraCore.onDom(records => {
        const inside = records.filter(r => r.target instanceof Node && main.contains(r.target));
        if (inside.length) onRecords(inside);
      });
      return;
    }
    new MutationObserver(onRecords).observe(main, { childList: true, subtree: true });
  }

  let drawing = null;
  let rerun = false;
  /** The next box to leave on its own. @type {ReturnType<typeof setTimeout>|undefined} */
  let leaving;
  /** The side and bottom bento containers, kept when the conversation redraws. @type {?HTMLElement} */
  let kept = null;
  /** @type {?HTMLElement} */
  let keptBottom = null;

  // Thread history compaction and completed-answer annotations live in aurora-stage-view.js.
  const compactThreadHistory = conv => typeof AuroraStageView !== "undefined" && AuroraStageView.compactThreadHistory(conv);
  const annotateAnswers = conv => typeof AuroraStageView !== "undefined" && AuroraStageView.annotateAnswers(conv, ended, named, firstLine, html);

  /** Creates a kept bento nav element with hover-hold behavior. @param {string} cls @param {string} label */
  function makeBento(cls, label) {
    const el = document.createElement("nav");
    el.className = cls;
    el.setAttribute("aria-label", label);
    sized.observe(el);
    const away = () => setTimeout(() => {
      if (!el.isConnected || el.matches(":hover, :focus-within")) return;
      AuroraBoxes.left(boxesIn(el).map(b => b.dataset.sign || ""));
      refresh();
    }, 0);
    el.addEventListener("pointerleave", away);
    el.addEventListener("focusout", away);
    return el;
  }

  /**
   * The primary Side Bento inside `section.conv`.
   * @param {Element} conv
   */
  function bentoIn(conv) {
    let bento = /** @type {?HTMLElement} */ (conv.querySelector(":scope > .stage-bento"));
    if (!bento) {
      if (!kept) { kept = makeBento("stage-bento", "Live workspace"); at = new WeakMap(); }
      bento = kept;
    }
    const thread = conv.querySelector(":scope > .thread");
    const before = conv.querySelector(":scope > .composer-slot");
    if (thread) { if (thread.nextElementSibling !== bento) thread.after(bento); }
    else if (before) { if (before.previousElementSibling !== bento) before.before(bento); }
    else if (bento.parentElement !== conv) conv.append(bento);
    return bento;
  }

  /**
   * The secondary Bottom Bento strip inside `section.conv` (parallel tasks,
   * sub-agents, fresh files, and recent work spanning below the chat & side).
   * @param {Element} conv @param {Element} sideBento
   */
  function bentoBottomIn(conv, sideBento) {
    let bottom = /** @type {?HTMLElement} */ (conv.querySelector(":scope > .stage-bento-bottom"));
    if (!bottom) {
      if (!keptBottom) keptBottom = makeBento("stage-bento-bottom", "Workspace activity");
      bottom = keptBottom;
    }
    if (sideBento.nextElementSibling !== bottom) sideBento.after(bottom);
    return bottom;
  }

  /** Composes the autonomous stage inside `section.conv`, rearranging tiles across Side & Bottom bento. */
  function refresh() {
    const main = document.getElementById("main");
    if (!main) return Promise.resolve();
    watch(main);
    const conv = main.querySelector(":scope > section.conv");
    if (!on() || !conv) {
      document.querySelectorAll(".stage-bento, .stage-bento-bottom").forEach(b => b.remove());
      document.querySelectorAll(".aurora-canvas-row").forEach(r => r.remove());
      if (conv instanceof HTMLElement) { delete conv.dataset.stageMode; delete conv.dataset.hasBottom; }
      return Promise.resolve();
    }
    compactThreadHistory(conv);
    if (drawing) { rerun = true; return drawing; }
    drawing = (async () => {
      const all = await boxes();
      const now = main.querySelector(":scope > section.conv");
      if (!now) return;
      compactThreadHistory(now);
      const bento = bentoIn(now);
      const bottom = bentoBottomIn(now, bento);
      const heldSet = new Set([
        ...(bento.matches(":hover, :focus-within") ? boxesIn(bento).map(b => b.dataset.sign || "") : []),
        ...(bottom.matches(":hover, :focus-within") ? boxesIn(bottom).map(b => b.dataset.sign || "") : []),
      ]);
      const visible = AuroraBoxes.shown(all, Date.now(), heldSet);
      const list = visible.slice(0, SHOWN_BOXES);
      const extra = Math.max(0, visible.length - list.length);
      let sideList = list;
      let bottomList = [];
      if (list.length > 2) {
        const isPrimary = c => c.key === "live" || c.key.startsWith("approval:");
        const primary = list.filter(isPrimary);
        sideList = primary.length ? primary.slice(0, 2) : list.slice(0, 2);
        const sideKeys = new Set(sideList.map(c => c.key));
        bottomList = list.filter(c => !sideKeys.has(c.key));
      }
      if (now instanceof HTMLElement) {
        now.dataset.stageMode = list.length ? "composed" : "chat";
        now.dataset.hasBottom = bottomList.length ? "1" : "0";
      }
      place(bento, sideList);
      place(bottom, bottomList);
      const overflowHost = bottomList.length ? bottom : bento;
      document.querySelectorAll(".stage-bento-overflow").forEach(o => { if (o.parentElement !== overflowHost) o.remove(); });
      let over = overflowHost.querySelector(":scope > .stage-bento-overflow");
      if (extra > 0) {
        if (!over) { over = document.createElement("a"); over.className = "stage-bento-overflow"; over.setAttribute("href", "#/tasks"); over.setAttribute("data-open", "tasks"); overflowHost.append(over); }
        over.textContent = `+${extra} more active in workspace sidebar →`;
      } else if (over) {
        over.remove();
      }
      clearTimeout(leaving);
      const wait = AuroraBoxes.next(list);
      if (wait !== null) leaving = setTimeout(refresh, wait + LEAVE_SLACK_MS);
      annotateAnswers(now);
    })().catch(() => { /* The stage waits for the next change. */ }).finally(() => {
      drawing = null;
      if (rerun) { rerun = false; refresh(); }
    });
    return drawing;
  }

  /**
   * The column beside the conversation: the canvas for what is open.
   * @param {{ kind: string, title: string, head?: string|null, backBtn: string, tabsHtml?: string, body: string, foot?: string }} v
   */
  const shell = v => typeof AuroraStageView !== "undefined" ? AuroraStageView.shell(v, html) : "";
  const decision = body => typeof AuroraStageView !== "undefined" ? AuroraStageView.decision(body, html) : null;

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

  const stream = (block, startIndex = 0) => typeof AuroraStageView !== "undefined" ? AuroraStageView.stream(block, startIndex, on(), moving()) : startIndex;

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

  return { on, refresh, boxes, shell, settle, closed, stream, takes, notes };
})();
globalThis.AuroraStage = AuroraStage;
