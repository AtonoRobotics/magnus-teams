// Full-stage Morning Briefing & Quiet Routines view (#/briefing) for Magnus.
"use strict";

const NovaPages = (() => {
  // How long the Briefing's snooze keeps notifications quiet.
  const BRIEF_SNOOZE_MINUTES = 120;
  // The briefing's summary, written from what /v1/briefing reports: what
  // finished in the last day, what is staged, commitments overdue and the
  // next routine. Nothing to say gives no summary.
  function briefSummary(brief) {
    const st = brief?.stats;
    if (!st) return "";
    const n = (k, one, many) => `${k} ${k === 1 ? one : many}`;
    const parts = [];
    if (st.completed_24h) parts.push(`${n(st.completed_24h, "task", "tasks")} finished in the last day`);
    if (st.active_count) parts.push(`${n(st.active_count, "task is", "tasks are")} working now`);
    if (st.staged_count) parts.push(`${n(st.staged_count, "draft waits", "drafts wait")} to be sent`);
    if (st.overdue_count) parts.push(`${n(st.overdue_count, "commitment is", "commitments are")} overdue`);
    // The upcoming list carries ids only; a routine's title is in App.schedules.
    const next = (brief.upcoming_schedules || []).map(u => ({ ...u, sc: (App.schedules || []).find(s => s.id === u.id) })).find(u => u.sc && !u.sc.system);
    if (next) parts.push(`next up: ${next.sc.title || "a routine"}${next.next && typeof until === "function" ? ` ${until(next.next)}` : ""}`);
    if (!parts.length) return "";
    const s = parts.join(" · ");
    return s.charAt(0).toUpperCase() + s.slice(1) + ".";
  }

  async function showBriefing(main) {
    if (typeof closePanel === "function" && Panel?.what) closePanel(false);
    App.thread = null;
    if (typeof sidebar === "function") sidebar();

    const draw = async () => {
      const pending = typeof needsMe === "function" ? needsMe() : [];
      // The person's own routines that still run: a system schedule (the
      // retired heartbeat) or an ended one is not a routine to run.
      const scheds = (App.schedules || []).filter(s => !s.deleted && !s.ended && !s.system);
      const activeTasks = (App.tasks || []).filter(t => typeof ENDED !== "undefined" ? !ENDED.includes(t.status) : t.status === "running");
      let brief = null, spend = null;
      try {
        if (typeof Server !== "undefined" && Server.call) [brief, spend] = await Promise.all([Server.call("GET", "/v1/briefing"), Server.spend?.get?.().catch(() => null)]);
      } catch { /* optional endpoint */ }
      const dnd = typeof Notices !== "undefined" && Notices.isDnd?.();
      const usd = v => `$${Number(v || 0).toFixed(2)}`;
      const spendCard = spend && Number.isFinite(spend.spent_usd)
        ? `<div class="card"><span class="small muted">Spend This Month</span><strong class="nova-metric-val">${esc(usd(spend.spent_usd))}</strong><span class="small muted">${Number.isFinite(spend.limit_usd) ? `of ${esc(usd(spend.limit_usd))} limit` : "No limit set"}</span></div>`
        : "";

      const headline = pending.length
        ? `${pending.length} ${pending.length === 1 ? "decision waits" : "decisions wait"} on you`
        : "All quiet — nothing needs your attention";

      const routineRows = scheds.length
        ? scheds.map(s => `<div class="nova-routine-row" data-routine-id="${esc(s.id)}">
            <div class="nova-routine-main">
              <strong>${esc(s.title || s.name || "Routine")}</strong>
              <span class="small muted">${esc(typeof rule === "function" ? rule(s) : (s.daily_at || "Scheduled"))}${s.paused ? " · Paused" : s.next ? ` · Next ${esc(typeof until === "function" ? until(s.next) : s.next)}` : ""}</span>
            </div>
            <div class="row">
              <button type="button" class="small" data-routine-dry="${esc(s.id)}">Dry run</button>
              ${s.paused ? "" : `<button type="button" class="primary small" data-routine-run="${esc(s.id)}">Run now</button>`}
            </div>
          </div>`).join("")
        : `<p class="empty">No quiet routines scheduled yet. Ask Magnus to run any check on a schedule.</p>`;

      const summary = briefSummary(brief);
      const summaryHtml = summary
        ? `<div class="card nova-brief-summary"><p>${esc(summary)}</p></div>`
        : "";

      paint(main, `<div class="nova-briefing-page">
        <header class="nova-brief-head">
          <div>
            <span class="small muted">Executive Morning Briefing</span>
            <h1>${esc(headline)}</h1>
          </div>
          <div class="row">
            <button type="button" class="small" data-brief-dnd aria-pressed="${dnd ? "true" : "false"}">${dnd ? "Turn notifications back on" : "Snooze notifications 2h"}</button>
            ${pending.length ? `<a class="button primary" href="#/inbox">Review ${pending.length} waiting</a>` : `<a class="button primary" href="#/chat/new">New conversation</a>`}
          </div>
        </header>
        ${summaryHtml}
        <div class="nova-brief-metrics">
          <div class="card"><span class="small muted">Pending Decisions</span><strong class="nova-metric-val">${pending.length}</strong><span class="small muted">${pending.length ? "Waiting for your approval" : "Queue clear"}</span></div>
          <div class="card"><span class="small muted">Quiet Routines</span><strong class="nova-metric-val">${scheds.length}</strong><span class="small muted">Run silently in background</span></div>
          <div class="card"><span class="small muted">Active Tasks</span><strong class="nova-metric-val">${activeTasks.length}</strong><span class="small muted">${activeTasks.length ? "Working now" : "Standing by"}</span></div>
          ${spendCard}
        </div>
        <section class="nova-brief-sec">
          <h2>Quiet Routines</h2>
          <div class="nova-routine-list">${routineRows}</div>
        </section>
      </div>`);
    };

    await draw();

    main.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-brief-dnd]") && typeof Notices !== "undefined") {
        const on = Notices.isDnd();
        Notices.snooze(on ? 0 : BRIEF_SNOOZE_MINUTES);
        toast(on ? "Notifications are back on." : "Notifications snoozed for 2 hours. Approvals still wait in Inbox.");
        await draw();
        return;
      }
      const runBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-routine-run]"));
      if (runBtn) {
        const id = runBtn.dataset.routineRun || "";
        runBtn.disabled = true;
        try {
          await Server.schedules.run(id);
          toast("Routine started.");
          if (typeof refresh === "function") await refresh();
          await draw();
        } catch (err) { toast(err.message); runBtn.disabled = false; }
        return;
      }
      const dryBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-routine-dry]"));
      if (dryBtn) {
        const id = dryBtn.dataset.routineDry || "";
        dryBtn.disabled = true;
        try {
          const r = await Server.schedules.run(id, { dry_run: true });
          const what = String(r?.prompt_preview || "").split("\n")[0].slice(0, 160);
          toast(r?.would_run === false ? "Dry run: this routine is paused, so it would not run now." : `Dry run, nothing started: it would ask “${what}”.`);
        } catch (err) { toast(err.message); }
        dryBtn.disabled = false;
      }
    };
  }

  return { showBriefing };
})();
