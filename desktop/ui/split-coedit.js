// Split-Screen Co-Edit Canvas (Document + Tracked Diff Hunks left, Magnus companion right).
// Builds directly on coedit.js primitives for tracked suggestions and conflict-safe editing.
"use strict";

const SplitCoedit = (() => {
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /**
   * @typedef {Object} DiffHunk
   * @property {string} id
   * @property {string} heading
   * @property {string} before
   * @property {string} after
   * @property {"pending" | "accepted" | "rejected"} status
   */

  /**
   * Compute paragraph-level diff hunks between two versions of a document.
   * @param {string} beforeText
   * @param {string} afterText
   * @returns {DiffHunk[]}
   */
  function diffHunks(beforeText, afterText) {
    const a = String(beforeText || "").split(/\n{2,}/).map(s => s.trim()).filter(Boolean);
    const b = String(afterText || "").split(/\n{2,}/).map(s => s.trim()).filter(Boolean);
    const n = Math.max(a.length, b.length);
    /** @type {DiffHunk[]} */
    const out = [];
    for (let i = 0; i < n; i++) {
      const left = a[i] || "";
      const right = b[i] || "";
      if (left !== right) {
        out.push({ id: `hunk-${i + 1}`, heading: `Section ${i + 1}`, before: left, after: right, status: "pending" });
      }
    }
    return out;
  }

  /**
   * Resolve a single hunk ("accept" or "reject") and return updated list.
   * @param {DiffHunk[]} hunks @param {string} id @param {"accept" | "reject"} decision
   * @returns {DiffHunk[]}
   */
  function resolveHunk(hunks, id, decision) {
    const nextStatus = decision === "accept" ? "accepted" : "rejected";
    return (hunks || []).map(h => (h.id === id ? { ...h, status: nextStatus } : h));
  }

  /**
   * Reconstruct the merged document text from base content + resolved/pending hunks.
   * @param {string} baseContent @param {DiffHunk[]} hunks
   */
  function buildDocument(baseContent, hunks) {
    let text = String(baseContent || "");
    for (const h of (hunks || [])) {
      if (h.status === "accepted") {
        if (h.before && text.includes(h.before)) text = text.replace(h.before, h.after);
        else text = text ? `${text}\n\n${h.after}` : h.after;
      }
    }
    return text.trim();
  }

  /**
   * Open the interactive Split-Screen Co-Edit modal.
   * @param {{ title?: string, content?: string, hunks?: DiffHunk[], onSave?: (text: string) => Promise<any>, onSend?: (prompt: string) => void }} [o]
   */
  function open(o = {}) {
    if (typeof document === "undefined") return null;
    const existing = document.querySelector(".split-coedit-dialog");
    if (existing) existing.remove();

    let docText = String(o.content || "");
    /** @type {DiffHunk[]} */
    let hunks = Array.isArray(o.hunks) ? o.hunks.map(h => ({ ...h })) : [];
    const title = String(o.title || "Untitled draft");
    /** @type {Array<{ who: string, text: string }>} */
    const notes = [{ who: "Magnus", text: "Edit directly on the left, or ask me below to propose tracked diff hunks you can accept or reject." }];

    const d = document.createElement("dialog");
    d.className = "split-coedit-dialog";
    d.setAttribute("aria-label", `Split Co-Edit: ${title}`);

    const draw = () => {
      const pending = hunks.filter(h => h.status === "pending").length;
      const hunksHtml = hunks.map(h => {
        if (h.status === "pending") {
          return `<div class="sc-hunk" data-sc-hunk="${escHtml(h.id)}">
            <div class="sc-hunk-head"><strong>${escHtml(h.heading)}</strong><div class="row"><button type="button" class="small primary" data-sc-act="accept" data-sc-id="${escHtml(h.id)}">Accept</button><button type="button" class="small" data-sc-act="reject" data-sc-id="${escHtml(h.id)}">Reject</button></div></div>
            ${h.before ? `<div class="sc-diff-del">${escHtml(h.before)}</div>` : ""}
            <div class="sc-diff-add">${escHtml(h.after)}</div>
          </div>`;
        }
        return `<div class="sc-hunk sc-hunk-done" data-sc-hunk="${escHtml(h.id)}"><span class="small muted">${escHtml(h.heading)} · ${h.status === "accepted" ? "Accepted" : "Kept original"}</span></div>`;
      }).join("");

      d.innerHTML = `<div class="sc-shell">
        <header class="sc-head">
          <div class="row"><strong class="sc-title">${escHtml(title)}</strong><span class="chip">${pending ? `${pending} pending hunk${pending === 1 ? "" : "s"}` : "Synced"}</span></div>
          <div class="row">
            ${pending ? `<button type="button" class="small primary" data-sc-all="accept">Accept all</button><button type="button" class="small" data-sc-all="reject">Reject all</button>` : ""}
            <button type="button" class="small" data-sc-copy>Copy</button>
            ${o.onSave ? `<button type="button" class="small primary" data-sc-save>Save</button>` : ""}
            <button type="button" class="small" data-sc-close aria-label="Close Split Co-Edit">Close</button>
          </div>
        </header>
        <div class="sc-panes">
          <div class="sc-left">
            <textarea class="sc-editor" data-sc-editor aria-label="Document editor">${escHtml(docText)}</textarea>
            ${hunksHtml ? `<div class="sc-hunks">${hunksHtml}</div>` : ""}
          </div>
          <aside class="sc-right">
            <div class="sc-log">${notes.map(n => `<div class="sc-note"><div class="small muted">${escHtml(n.who)}</div><div>${escHtml(n.text)}</div></div>`).join("")}</div>
            <div class="sc-ask">
              <input type="text" data-sc-prompt placeholder="Ask Magnus to propose an edit…" aria-label="Ask Magnus to edit this document">
              <button type="button" class="small primary" data-sc-propose>Propose</button>
            </div>
          </aside>
        </div>
      </div>`;
    };

    const syncEditor = () => {
      const ed = /** @type {?HTMLTextAreaElement} */ (d.querySelector("[data-sc-editor]"));
      if (ed) docText = ed.value;
    };

    const proposeEdit = () => {
      syncEditor();
      const inp = /** @type {?HTMLInputElement} */ (d.querySelector("[data-sc-prompt]"));
      const req = inp?.value?.trim() || "";
      if (!req) return;
      notes.push({ who: "You", text: req });
      const firstPara = docText.split(/\n{2,}/)[0] || "";
      const nextId = `hunk-${hunks.length + 1}`;
      hunks.push({
        id: nextId,
        heading: `Proposed edit (${req.slice(0, 32)})`,
        before: firstPara,
        after: firstPara ? `${firstPara}\n(${req})` : req,
        status: "pending",
      });
      notes.push({ who: "Magnus", text: `Staged a tracked diff hunk for: "${req}". Review and Accept or Reject on the left.` });
      if (typeof o.onSend === "function") o.onSend(`Edit "${title}": ${req}`);
      draw();
    };

    d.addEventListener("click", async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t === d || t.closest?.("[data-sc-close]")) { d.close(); return; }
      const actBtn = t.closest?.("[data-sc-act]");
      if (actBtn) {
        syncEditor();
        const id = actBtn.dataset.scId;
        const act = /** @type {"accept" | "reject"} */ (actBtn.dataset.scAct);
        hunks = resolveHunk(hunks, id, act);
        if (act === "accept") docText = buildDocument(docText, hunks.filter(h => h.id === id));
        draw();
        return;
      }
      const allBtn = t.closest?.("[data-sc-all]");
      if (allBtn) {
        syncEditor();
        const act = /** @type {"accept" | "reject"} */ (allBtn.dataset.scAll);
        const pendingIds = hunks.filter(h => h.status === "pending").map(h => h.id);
        for (const id of pendingIds) hunks = resolveHunk(hunks, id, act);
        if (act === "accept") docText = buildDocument(docText, hunks.filter(h => pendingIds.includes(h.id)));
        draw();
        return;
      }
      if (t.closest?.("[data-sc-copy]")) {
        syncEditor();
        if (typeof copyText === "function") await copyText(docText);
        if (typeof toast === "function") toast("Copied document.");
        return;
      }
      if (t.closest?.("[data-sc-save]") && o.onSave) {
        syncEditor();
        await o.onSave(docText);
        if (typeof toast === "function") toast("Saved.");
        return;
      }
      if (t.closest?.("[data-sc-propose]")) { proposeEdit(); }
    });

    d.addEventListener("keydown", e => {
      if (e.key === "Enter" && /** @type {HTMLElement} */ (e.target).closest?.("[data-sc-prompt]")) {
        e.preventDefault();
        proposeEdit();
      }
    });

    d.addEventListener("close", () => d.remove());
    draw();
    document.body.append(d);
    d.showModal();
    return d;
  }

  const api = { diffHunks, resolveHunk, buildDocument, open };
  if (typeof globalThis !== "undefined") globalThis.SplitCoedit = api;
  return api;
})();
