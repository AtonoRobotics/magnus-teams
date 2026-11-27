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
      let brief = null, spend = null, commitments = [], suggestions = [];
      try {
        if (typeof Server !== "undefined" && Server.call) {
          const [b, sp, cmt, sug] = await Promise.all([
            Server.call("GET", "/v1/briefing"),
            Server.spend?.get?.().catch(() => null),
            Server.memory?.commitments?.().catch(() => null),
            Server.rules?.suggestions?.().catch(() => null),
          ]);
          brief = b;
          spend = sp;
          commitments = (cmt?.commitments || []).filter(c => c.status === "open").slice(0, 5);
          suggestions = (sug?.suggestions || []).slice(0, 4);
        }
      } catch { /* optional endpoint */ }
      const dnd = typeof Notices !== "undefined" && Notices.isDnd?.();
      const usd = v => `$${Number(v || 0).toFixed(2)}`;
      const spendCard = spend && Number.isFinite(spend.spent_usd)
        ? `<div class="card"><span class="small muted">Spend This Month</span><strong class="nova-metric-val">${esc(usd(spend.spent_usd))}</strong><span class="small muted">${Number.isFinite(spend.limit_usd) ? `of ${esc(usd(spend.limit_usd))} limit` : "No limit set"}</span></div>`
        : "";

      const headline = pending.length
        ? `${pending.length} ${pending.length === 1 ? "decision waits" : "decisions wait"} on you`
        : "All quiet — nothing needs your attention";

      const batchIds = typeof batchableApprovals === "function" ? batchableApprovals(pending) : [];
      const approvalsHtml = pending.length && typeof askCard === "function"
        ? `<section class="nova-brief-sec" data-brief-approvals>
            <div class="spread"><h2>Needs your decision now</h2>${batchIds.length >= 2 ? `<button type="button" class="primary small" data-brief-batch>Allow all ${batchIds.length}</button>` : ""}</div>
            <div class="stack">${pending.slice(0, 3).map(i => `<div class="card flat">${askCard(i, i.can_respond !== false)}</div>`).join("")}</div>
            ${pending.length > 3 ? `<p class="small"><a href="#/inbox">View all ${pending.length} waiting in Inbox →</a></p>` : ""}
          </section>`
        : "";

      const trustHtml = suggestions.length
        ? `<section class="nova-brief-sec" data-brief-trust>
            <h2>Suggested standing rules</h2>
            <div class="stack">${suggestions.map((sg, idx) => `<div class="card flat spread" data-brief-sug="${idx}"><div>
              <strong>${esc(sg.tool.replace(/^[^_]+__/, "").replace(/_/g, " "))}</strong> in <span class="mono">${esc(sg.connector)}</span> → <span class="mono">${esc(sg.destination)}</span>
              <div class="small muted">Approved ${esc(String(sg.count))} times</div></div>
              <button type="button" class="primary small" data-brief-graduate="${idx}">Allow automatically</button></div>`).join("")}</div>
          </section>`
        : "";

      const DAY_MS = 24 * 60 * 60 * 1000;
      const done24h = (App.tasks || []).filter(t => t.status === "completed" && !t.routine && !t.system && t.ended_at && (Date.now() - Date.parse(t.ended_at)) < DAY_MS).slice(0, 5);
      const completedHtml = done24h.length
        ? `<section class="nova-brief-sec" data-brief-completed>
            <h2>Completed in the last 24 hours</h2>
            <div class="stack">${done24h.map(t => `<a class="card flat spread" href="#/task/${enc(t.task_id)}"><div>
              <strong>${esc(t.title || "Completed task")}</strong>
              ${t.summary ? `<div class="small muted">${esc(String(t.summary).split("\n")[0].slice(0, 140))}</div>` : ""}</div>
              <span class="small muted num">${t.receipts_count ? `${t.receipts_count} verified` : "Done"}</span></a>`).join("")}</div>
          </section>`
        : "";

      const commitmentsHtml = commitments.length
        ? `<section class="nova-brief-sec" data-brief-commitments>
            <h2>Open commitments</h2>
            <div class="stack">${commitments.map(c => `<div class="card flat spread" data-brief-cmt="${esc(c.id)}"><div>
              <strong>${esc(c.what || "")}</strong>
              <div class="small muted">${c.to ? `For ${esc(c.to)}` : ""}${c.by ? `${c.to ? " · " : ""}By ${esc(c.by)}` : ""}</div></div>
              <button type="button" class="small" data-brief-cmt-done="${esc(c.id)}">Mark done</button></div>`).join("")}</div>
          </section>`
        : "";

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
        ? `<p class="nova-brief-summary">${esc(summary)}</p>`
        : "";

      paint(main, `<div class="nova-briefing-page">
        <header class="nova-brief-head">
          <div>
            <span class="small muted">Executive Morning Briefing</span>
            <h1>${esc(headline)}</h1>
            ${summaryHtml}
          </div>
          <div class="row">
            <button type="button" class="small" data-brief-dnd aria-pressed="${dnd ? "true" : "false"}">${dnd ? "Turn notifications back on" : "Snooze notifications 2h"}</button>
            ${pending.length ? `<a class="button primary small" href="#/inbox">Review ${pending.length} waiting</a>` : ""}
          </div>
        </header>
        <div class="nova-brief-metrics">
          <div class="card"><span class="small muted">Pending Decisions</span><strong class="nova-metric-val">${pending.length}</strong><span class="small muted">${pending.length ? "Waiting for your approval" : "Queue clear"}</span></div>
          <div class="card"><span class="small muted">Quiet Routines</span><strong class="nova-metric-val">${scheds.length}</strong><span class="small muted">Run silently in background</span></div>
          ${spendCard}
        </div>
        ${approvalsHtml}
        ${trustHtml}
        ${completedHtml}
        ${commitmentsHtml}
        <section class="nova-brief-sec">
          <h2>Quiet Routines</h2>
          <div class="nova-routine-list">${routineRows}</div>
        </section>
      </div>`);

      main.onclick = async e => {
        const t = /** @type {HTMLElement} */ (e.target);
        if (t.closest?.("[data-brief-dnd]") && typeof Notices !== "undefined") {
          const on = Notices.isDnd();
          Notices.snooze(on ? 0 : BRIEF_SNOOZE_MINUTES);
          toast(on ? "Notifications are back on." : "Notifications snoozed for 2 hours. Approvals still wait in Inbox.");
          await draw();
          return;
        }
        const decideBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-item][data-decision]"));
        if (decideBtn && typeof respond === "function") {
          await respond(decideBtn, main);
          await draw();
          return;
        }
        if (t.closest?.("[data-brief-batch]") && batchIds.length) {
          const b = /** @type {HTMLButtonElement} */ (t.closest("[data-brief-batch]"));
          b.disabled = true;
          try {
            await Server.inbox.batch(batchIds, { decision: "approve", scope: "once" });
            toast(`Allowed ${batchIds.length} items.`);
            if (typeof refresh === "function") await refresh();
            await draw();
          } catch (err) { toast(err.message); b.disabled = false; }
          return;
        }
        const gradBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-brief-graduate]"));
        if (gradBtn) {
          const sg = suggestions[Number(gradBtn.dataset.briefGraduate)];
          if (!sg) return;
          gradBtn.disabled = true;
          try {
            await Server.rules.save(null, { connector: sg.connector, tool: sg.tool, destinations: [sg.destination], effect: "allow", words: sg.words });
            toast("Standing rule saved.");
            await draw();
          } catch (err) { toast(err.message); gradBtn.disabled = false; }
          return;
        }
        const cmtBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-brief-cmt-done]"));
        if (cmtBtn) {
          cmtBtn.disabled = true;
          try {
            await Server.memory.updateCommitment(cmtBtn.dataset.briefCmtDone, { status: "done" });
            toast("Marked done.");
            await draw();
          } catch (err) { toast(err.message); cmtBtn.disabled = false; }
          return;
        }
        if (t.closest?.("[data-dry-dismiss]")) {
          t.closest("[data-dry-preview]")?.remove();
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
            const full = String(r?.prompt_preview || "").trim();
            const what = full.split("\n")[0].slice(0, 160);
            const row = dryBtn.closest?.("[data-routine-id]");
            row?.parentElement?.querySelector(`[data-dry-preview="${CSS.escape(id)}"]`)?.remove();
            if (row && typeof document !== "undefined") {
              const box = document.createElement("div");
              box.className = "card flat";
              box.setAttribute("data-dry-preview", id);
              box.innerHTML = `<div class="spread"><strong>${r?.would_run === false ? "Dry run · Paused (would not run)" : "Dry run preview · Nothing started"}</strong><button type="button" class="link small" data-dry-dismiss>Dismiss</button></div>
                ${full ? `<p class="small muted">${esc(full)}</p>` : ""}`;
              row.insertAdjacentElement("afterend", box);
            }
            toast(r?.would_run === false ? "Dry run: this routine is paused, so it would not run now." : `Dry run, nothing started: it would ask “${what}”.`);
          } catch (err) { toast(err.message); }
          dryBtn.disabled = false;
        }
      };
    };

    await draw();
  }

  return { showBriefing };
})();
