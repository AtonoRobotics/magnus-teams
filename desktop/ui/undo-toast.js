// Reusable 5-second optimistic Undo toast for non-destructive triage actions
// (dismiss, defer, archive). Cancels the pending mutation if Undo is clicked;
// flushes pending commits on page unload so nothing is silently lost.
"use strict";

const UndoToast = (() => {
  const UNDO_WINDOW_MS = 5000;
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /** @type {{ timer: any, commit: () => any, undo: () => any, el: HTMLElement | null } | null} */
  let pending = null;
  let unloadWired = false;

  function ensureUnload() {
    if (unloadWired || typeof window === "undefined") return;
    unloadWired = true;
    window.addEventListener("beforeunload", flush);
  }

  function clearEl() {
    if (pending?.el) pending.el.remove();
  }

  /** Immediately commit any pending optimistic action. */
  function flush() {
    if (!pending) return;
    const cur = pending;
    pending = null;
    clearTimeout(cur.timer);
    if (cur.el) cur.el.remove();
    try { cur.commit(); } catch { /* ignore unload errors */ }
  }

  /** Cancel the active optimistic action and restore UI state. */
  function cancel() {
    if (!pending) return false;
    const cur = pending;
    pending = null;
    clearTimeout(cur.timer);
    if (cur.el) cur.el.remove();
    cur.undo();
    return true;
  }

  /**
   * Schedule an optimistic action with a 5-second Undo window.
   * @param {{ label: string, commit: () => any, undo: () => any, delayMs?: number }} o
   */
  function schedule(o) {
    ensureUnload();
    if (pending) flush();
    const delayMs = Number.isFinite(o.delayMs) ? Math.max(0, o.delayMs) : UNDO_WINDOW_MS;
    let el = null;
    if (typeof document !== "undefined" && document.body) {
      el = document.createElement("div");
      el.className = "undo-toast";
      el.setAttribute("role", "status");
      el.setAttribute("aria-live", "polite");
      el.innerHTML = `<div class="undo-toast-row"><span class="undo-toast-msg">${escHtml(o.label)}</span><button type="button" class="small primary" data-undo-act>Undo</button></div><div class="undo-toast-bar"><div class="undo-toast-fill" style="animation-duration:${delayMs}ms"></div></div>`;
      el.querySelector("[data-undo-act]")?.addEventListener("click", () => cancel());
      document.body.append(el);
    }
    const timer = setTimeout(() => {
      if (!pending || pending.timer !== timer) return;
      const cur = pending;
      pending = null;
      if (cur.el) cur.el.remove();
      Promise.resolve().then(() => cur.commit()).catch(err => {
        cur.undo();
        if (typeof toast === "function") toast(err?.message || "Action failed; restored.");
      });
    }, delayMs);
    pending = { timer, commit: o.commit, undo: o.undo, el };
  }

  return { UNDO_WINDOW_MS, schedule, cancel, flush, isPending: () => !!pending };
})();
