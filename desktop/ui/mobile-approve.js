// Out-of-Band Mobile / Async Approval Channel (Tier 3 Phase 2):
// Dedicated thumb-friendly triage view at #/approve (and #/approve?ask=<id>).
// Reuses asks.js askCard() verbatim so server-authored destination previews,
// risk tier badges, and Edit-before-allowing preserve all security invariants.
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
    const risk = String(ask?.risk || ask?.tier || "").toLowerCase();
    return risk !== "irreversible" && risk !== "financial" && !ask?.financial && !ask?.irreversible;
  }

  /** Render the mobile-first approval triage screen.
   * @param {HTMLElement} main @param {string} [qs] */
  async function show(main, qs) {
    const focusId = parseAskId(qs);

    const loadAsks = async () => {
      if (typeof Server !== "undefined" && Server.asks?.list) {
        try {
          const r = await Server.asks.list();
          const list = Array.isArray(r) ? r : (r?.asks || r?.items || []);
          if (typeof App !== "undefined") App.asks = list;
          if (typeof UIStore !== "undefined") UIStore.patch({ asks: list });
          return list;
        } catch { /* fall through to cached asks */ }
      }
      if (typeof UIStore !== "undefined" && Array.isArray(UIStore.get().asks)) return UIStore.get().asks;
      return typeof App !== "undefined" && Array.isArray(App.asks) ? App.asks : [];
    };

    const draw = async () => {
      const all = (await loadAsks()).filter(a => a && (!a.status || a.status === "pending" || a.status === "waiting"));
      const ordered = focusId
        ? [...all.filter(a => a.id === focusId), ...all.filter(a => a.id !== focusId)]
        : all;
      const reversible = ordered.filter(isReversibleAsk);

      const cards = ordered.length
        ? ordered.map(a => {
            const focused = focusId && a.id === focusId ? `<div class="small muted" style="margin-bottom:4px">Focused from notification</div>` : "";
            const body = typeof askCard === "function"
              ? askCard(a, true)
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

      const html = `<section class="mobile-approve-shell stack" data-mobile-approve aria-label="Mobile approval triage">
        <header class="mobile-approve-head row">
          <div>
            <h2>Approvals${ordered.length ? ` (${ordered.length})` : ""}</h2>
            <p class="small muted">Review destinations and risk before Magnus acts.</p>
          </div>
          <div class="row">
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
      const batch = t.closest?.("[data-mobile-allow-reversible]");
      if (batch) {
        const list = (typeof App !== "undefined" && Array.isArray(App.asks) ? App.asks : []).filter(isReversibleAsk);
        /** @type {HTMLButtonElement} */ (batch).disabled = true;
        for (const a of list) {
          if (typeof respond === "function") await respond(a.id, "allow_once");
          else if (typeof Server !== "undefined" && Server.asks?.respond) await Server.asks.respond(a.id, { decision: "allow_once" }).catch(() => {});
        }
        await draw();
        return;
      }
    };

    if (typeof App !== "undefined") App.redraw = draw;
    await draw();
  }

  return { parseAskId, approveUrl, isReversibleAsk, show };
})();
