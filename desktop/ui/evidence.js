// A task's evidence (spec rev 147, Evidence comes from the record): what the
// server's own log proves was done (receipts), what it read back after each
// irreversible action, the sources each claim links to, and what was not
// done. None of it rests on the agent's word; the server attaches it.
"use strict";

/** @typedef {{ call_id: string, tool: string, read?: string|null, found?: any, matches?: boolean|null, note: string }} ReadBack */
/** @typedef {{ call_id: string, tool: string, what: string, to: string[], at: string, read_back?: ReadBack|null }} Receipt */
/** @typedef {{ call_id: string, tool: string, status: string, what: string }} NotDone */
/** @typedef {{ claim: string, call_id: string, tool?: string|null, at?: string|null }} Source */
/** @typedef {{ receipts: Receipt[], not_done: NotDone[], sources: Source[], status_line?: string|null }} Evidence */

const NOT_DONE_SAID = { denied: "Not allowed", failed: "Failed", unknown: "Outcome unknown", read_back_differs: "Did not read back as done" };

// What the server read back, in one line, with what it found a tap away.
/** @param {ReadBack|null|undefined} rb */
function readBackLine(rb) {
  if (!rb) return "";
  const cls = rb.matches === true ? "ok" : rb.matches === false ? "warn" : "";
  const said = rb.matches === true ? "Read back: it matches" : rb.matches === false ? "Read back: it does not match" : "Not read back";
  const found = rb.found && Object.keys(rb.found).length ? `<details class="small"><summary>What was found</summary><pre>${esc(JSON.stringify(rb.found, null, 2))}</pre></details>` : "";
  return `<div class="small" data-read-back="${esc(rb.matches === true ? "matches" : rb.matches === false ? "differs" : "none")}"><span class="chip ${cls}">${esc(said)}</span> <span class="muted">${esc(rb.note)}</span>${found}</div>`;
}

/** @param {Receipt} r */
function receiptRow(r) {
  return `<li data-receipt="${esc(r.call_id)}"><div>${esc(r.what)}</div><div class="small faint">${esc(r.tool)} · ${esc(when(r.at))}</div>${readBackLine(r.read_back)}</li>`;
}

// The result's evidence, after the deliverable: receipts with their
// read-backs, sources, and anything not done.
/** @param {{ evidence?: Evidence }} t */
function evidenceSection(t) {
  const e = t.evidence;
  if (!e) return "";
  const parts = [];
  if (e.receipts?.length) parts.push(`<h3>What was done</h3><ul class="receipts" data-receipts>${e.receipts.map(receiptRow).join("")}</ul>`);
  if (e.sources?.length) {
    parts.push(`<h3>Sources</h3><ul class="sources" data-sources>${e.sources.map(s => `<li><span>${esc(s.claim)}</span> <span class="small faint">${esc(s.at || s.tool || "")}</span></li>`).join("")}</ul>`);
  }
  if (e.not_done?.length) {
    parts.push(`<h3>Not done</h3><ul class="not-done" data-not-done>${e.not_done.map(n => `<li><span class="chip warn">${esc(NOT_DONE_SAID[n.status] || n.status)}</span> ${esc(n.what)}</li>`).join("")}</ul>`);
  }
  return parts.join("");
}
