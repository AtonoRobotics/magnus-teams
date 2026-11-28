// Ephemeral inline comment manager for PlanSurface steps and staged drafts.
// Comments are keyed by (surfaceId, targetId) and compiled into one structured
// revision prompt when the person clicks "Revise with N comments".
"use strict";

const InlineComments = (() => {
  const escHtml = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  /** @type {Map<string, Map<string, string[]>>} */
  const store = new Map();

  /** @param {string} surfaceId */
  function bucket(surfaceId) {
    let m = store.get(surfaceId);
    if (!m) { m = new Map(); store.set(surfaceId, m); }
    return m;
  }

  /** @param {string} surfaceId @param {string} targetId @param {string} text */
  function add(surfaceId, targetId, text) {
    const clean = String(text || "").trim();
    if (!surfaceId || !targetId || !clean) return 0;
    const b = bucket(surfaceId);
    const list = b.get(targetId) || [];
    list.push(clean);
    b.set(targetId, list);
    return list.length;
  }

  /** @param {string} surfaceId @param {string} targetId @returns {string[]} */
  function list(surfaceId, targetId) {
    return [...(store.get(surfaceId)?.get(targetId) || [])];
  }

  /** @param {string} surfaceId @returns {number} */
  function count(surfaceId) {
    const b = store.get(surfaceId);
    if (!b) return 0;
    let total = 0;
    for (const arr of b.values()) total += arr.length;
    return total;
  }

  /** @param {string} surfaceId */
  function clear(surfaceId) {
    store.delete(surfaceId);
  }

  /**
   * Format all staged inline comments on a surface into one concise revision prompt.
   * @param {string} surfaceId
   * @param {Record<string, string>} [labels] optional map of targetId -> human label
   * @returns {string}
   */
  function formatRevision(surfaceId, labels = {}) {
    const b = store.get(surfaceId);
    if (!b || !b.size) return "";
    const lines = ["Revise the plan based on these inline comments:"];
    for (const [targetId, notes] of b.entries()) {
      const label = labels[targetId] || targetId;
      for (const note of notes) lines.push(`- [${label}]: ${note}`);
    }
    return lines.join("\n");
  }

  /** Render comment pills for one target. @param {string} surfaceId @param {string} targetId */
  function pillsHtml(surfaceId, targetId) {
    const notes = list(surfaceId, targetId);
    if (!notes.length) return "";
    return `<div class="ic-pills">${notes.map(n => `<span class="ic-pill">${escHtml(n)}</span>`).join("")}</div>`;
  }

  /** Render sticky revision footer bar when comments exist. @param {string} surfaceId */
  function footerHtml(surfaceId) {
    const n = count(surfaceId);
    if (!n) return "";
    return `<div class="ic-footer" data-ic-footer="${escHtml(surfaceId)}">
      <span class="small muted">${n} inline comment${n === 1 ? "" : "s"} staged</span>
      <div class="row">
        <button type="button" class="small" data-ic-clear="${escHtml(surfaceId)}">Clear</button>
        <button type="button" class="small primary" data-ic-revise="${escHtml(surfaceId)}">Revise with ${n} comment${n === 1 ? "" : "s"}</button>
      </div>
    </div>`;
  }

  return { add, list, count, clear, formatRevision, pillsHtml, footerHtml };
})();
