// Full-stage Morning Briefing & Quiet Routines view (#/briefing) for Magnus.
"use strict";

const NovaPages = (() => {
  async function showBriefing(main) {
    if (typeof closePanel === "function" && Panel?.what) closePanel(false);
    App.thread = null;
    if (typeof sidebar === "function") sidebar();

    const draw = async () => {
      const pending = typeof needsMe === "function" ? needsMe() : [];
      const scheds = (App.schedules || []).filter(s => !s.deleted);
      const activeTasks = (App.tasks || []).filter(t => typeof ENDED !== "undefined" ? !ENDED.includes(t.status) : t.status === "running");
      let brief = null;
      try {
        if (typeof Server !== "undefined" && Server.call) brief = await Server.call("GET", "/v1/briefing");
      } catch { /* optional endpoint */ }

      const headline = pending.length
        ? `${pending.length} ${pending.length === 1 ? "decision waits" : "decisions wait"} on you`
        : "All quiet — nothing needs your attention";

      const routineRows = scheds.length
        ? scheds.map(s => `<div class="nova-routine-row" data-routine-id="${esc(s.id)}">
            <div class="nova-routine-main">
              <strong>${esc(s.title || s.name || "Routine")}</strong>
              <span class="small muted">${esc(s.cadence || s.daily_at || "Scheduled")}${s.next ? ` · Next ${esc(typeof until === "function" ? until(s.next) : s.next)}` : ""}</span>
            </div>
            <div class="row">
              <button type="button" class="small" data-routine-dry="${esc(s.id)}">Dry run</button>
              <button type="button" class="primary small" data-routine-run="${esc(s.id)}">Run now</button>
            </div>
          </div>`).join("")
        : `<p class="empty">No quiet routines scheduled yet. Ask Magnus to run any check on a schedule.</p>`;

      const summaryHtml = brief?.summary
        ? `<div class="card nova-brief-summary"><p>${esc(brief.summary)}</p></div>`
        : "";

      paint(main, `<div class="nova-briefing-page">
        <header class="nova-brief-head">
          <div>
            <span class="small muted">Executive Morning Briefing</span>
            <h1>${esc(headline)}</h1>
          </div>
          <div class="row">
            ${pending.length ? `<a class="button primary" href="#/inbox">Review ${pending.length} waiting</a>` : `<a class="button primary" href="#/chat/new">New conversation</a>`}
          </div>
        </header>
        ${summaryHtml}
        <div class="nova-brief-metrics">
          <div class="card"><span class="small muted">Pending Decisions</span><strong class="nova-metric-val">${pending.length}</strong><span class="small muted">${pending.length ? "Waiting at Cedar gates" : "Queue clear"}</span></div>
          <div class="card"><span class="small muted">Quiet Routines</span><strong class="nova-metric-val">${scheds.length}</strong><span class="small muted">Run silently in background</span></div>
          <div class="card"><span class="small muted">Active Tasks</span><strong class="nova-metric-val">${activeTasks.length}</strong><span class="small muted">${activeTasks.length ? "Working now" : "Standing by"}</span></div>
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
      const runBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-routine-run]"));
      if (runBtn) {
        const id = runBtn.dataset.routineRun || "";
        runBtn.disabled = true;
        try {
          await Server.call("POST", `/v1/schedules/${enc(id)}/run`, {});
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
          const r = await Server.call("POST", `/v1/schedules/${enc(id)}/run`, { dry_run: true });
          toast(r?.summary || "Dry run previewed with zero mutations.");
        } catch (err) { toast(err.message); }
        dryBtn.disabled = false;
      }
    };
  }

  return { showBriefing };
})();
