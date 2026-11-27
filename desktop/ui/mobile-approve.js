// Out-of-Band Mobile / Async Approval Channel (Tier 3 Phase 2 + P0 Wiring Fix):
// Dedicated thumb-friendly triage view at #/approve (and #/approve?ask=<id>).
// Wired directly to real Magnus contracts: Server.inbox.list(), Server.inbox.batch(),
// needsMe(), askCard(a, true), respond(btn, main), and AdminState emergency stop.
"use strict";

const MobileApprove = (() => {
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /** Extract ?ask=<id> from a route query string. @param {string} [qs] */
  function parseAskId(qs) {
    if (!qs) return null;
    try {
      const params = new URLSearchParams(String(qs).replace(/^\?/, ""));
      return params.get("ask") || null;
    } catch { return null; }
  }

  /** Build a shareable deep link to the mobile approval view. @param {string} [askId] */
  function approveUrl(askId) {
    const base = typeof location !== "undefined" ? `${location.origin}${location.pathname}` : "/";
    return `${base}#/approve${askId ? `?ask=${encodeURIComponent(askId)}` : ""}`;
  }

  /** Whether an ask is eligible for batch reversible approval (never irreversible or financial).
   * @param {any} ask */
  function isReversibleAsk(ask) {
    const risk = String(ask?.tier || ask?.risk || "").toLowerCase();
    return risk !== "irreversible" && risk !== "financial" && !ask?.financial && !ask?.irreversible;
  }

  /** Filter inbox items down to open approvals waiting on the user. @param {any[]} items */
  function openApprovals(items) {
    if (!Array.isArray(items)) return [];
    if (typeof needsDecision === "function") return items.filter(needsDecision);
    return items.filter(a => a && a.status === "open" && a.can_respond !== false);
  }

  /** Render the mobile-first approval triage screen.
   * @param {HTMLElement} main @param {string} [qs] */
  async function show(main, qs) {
    const focusId = parseAskId(qs);

    const loadAsks = async () => {
      if (typeof Server !== "undefined" && Server.inbox?.list) {
        try {
          const r = await Server.inbox.list();
          const items = Array.isArray(r) ? r : (r?.items || []);
          if (typeof App !== "undefined") App.inbox = items;
          const pending = openApprovals(items);
          if (typeof UIStore !== "undefined") UIStore.patch({ asks: pending });
          return pending;
        } catch { /* fall through to cached inbox */ }
      }
      if (typeof needsMe === "function") return needsMe();
      if (typeof App !== "undefined" && Array.isArray(App.inbox)) return openApprovals(App.inbox);
      if (typeof UIStore !== "undefined" && Array.isArray(UIStore.get().asks)) return UIStore.get().asks;
      return [];
    };

    const draw = async () => {
      const all = await loadAsks();
      const ordered = focusId
        ? [...all.filter(a => a.id === focusId), ...all.filter(a => a.id !== focusId)]
        : all;
      const reversible = ordered.filter(isReversibleAsk);

      const cards = ordered.length
        ? ordered.map(a => {
            const focused = focusId && a.id === focusId ? `<div class="small muted" style="margin-bottom:4px">Focused from notification</div>` : "";
            const body = typeof askCard === "function"
              ? askCard(a, a.can_respond !== false)
              : `<div class="card ask" data-ask="${escHtml(a.id)}"><strong>${escHtml(a.summary || a.title || "Approval needed")}</strong></div>`;
            return `<div class="mobile-approve-item" data-mobile-ask="${escHtml(a.id)}">${focused}${body}</div>`;
          }).join("")
        : `<div class="card" data-mobile-approve-empty>
            <h3>All caught up</h3>
            <p class="small muted">Magnus has no actions waiting for your approval right now.</p>
            <div class="row" style="margin-top:10px">
              <a class="button" href="#/briefing">Morning Briefing</a>
              <a class="button" href="#/tasks">Open Tasks</a>
            </div>
          </div>`;

      const batchBtn = reversible.length >= 2
        ? `<button type="button" class="primary mobile-approve-batch" data-mobile-allow-reversible>Allow all ${reversible.length} reversible</button>`
        : "";
      const isAdmin = typeof AdminState !== "undefined" && !!AdminState.standing?.admin;
      const killBtn = isAdmin
        ? `<button type="button" class="danger small" data-mobile-admin-kill>Emergency Stop</button>`
        : "";

      const html = `<section class="mobile-approve-shell stack" data-mobile-approve aria-label="Mobile approval triage">
        <header class="mobile-approve-head row">
          <div>
            <h2>Approvals${ordered.length ? ` (${ordered.length})` : ""}</h2>
            <p class="small muted">Review destinations and risk before Magnus acts.</p>
          </div>
          <div class="row" style="flex-wrap:wrap;gap:6px">
            ${killBtn}
            ${batchBtn}
            <button type="button" class="small" data-mobile-refresh>Refresh</button>
            <a class="button small" href="#/briefing">Briefing</a>
          </div>
        </header>
        <div class="stack" data-mobile-approve-list>${cards}</div>
      </section>`;

      if (typeof paint === "function") paint(main, html);
      else main.innerHTML = html;
    };

    main.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-mobile-refresh]")) { await draw(); return; }
      if (t.closest?.("[data-mobile-admin-kill]")) {
        if (typeof adminStop === "function") await adminStop();
        else location.hash = "#/admin/operations";
        return;
      }
      const decisionBtn = /** @type {?HTMLButtonElement} */ (t.closest?.("button[data-decision]"));
      if (decisionBtn && typeof respond === "function") {
        await respond(decisionBtn, main);
        await draw();
        return;
      }
      const batch = /** @type {?HTMLButtonElement} */ (t.closest?.("[data-mobile-allow-reversible]"));
      if (batch) {
        const current = typeof needsMe === "function" ? needsMe() : openApprovals(typeof App !== "undefined" ? App.inbox : []);
        const ids = current.filter(isReversibleAsk).map(a => a.id);
        if (!ids.length) return;
        batch.disabled = true;
        try {
          if (typeof Server !== "undefined" && Server.inbox?.batch) {
            await Server.inbox.batch(ids, { decision: "approve", scope: "once" });
          } else if (typeof Server !== "undefined" && Server.inbox?.answer) {
            for (const id of ids) await Server.inbox.answer(id, { decision: "approve", scope: "once" });
          }
          if (typeof toast === "function") toast(`Allowed ${ids.length} reversible requests.`);
        } catch (err) {
          if (typeof toast === "function") toast(err?.message || "Batch approval failed.");
        }
        await draw();
      }
    };

    if (typeof App !== "undefined") App.redraw = draw;
    await draw();
  }

  return { parseAskId, approveUrl, isReversibleAsk, openApprovals, show };
})();
