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

/** Format concise scalar fields from a read-back payload for inline display. @param {any} found */
function readBackSummary(found) {
  if (!found || typeof found !== "object" || Array.isArray(found)) return "";
  const pairs = Object.entries(found)
    .filter(([, v]) => v !== null && v !== undefined && v !== "" && typeof v !== "object")
    .slice(0, 3)
    .map(([k, v]) => `${k.replace(/_/g, " ")}: ${String(v).slice(0, 80)}`);
  return pairs.length ? `<div class="small muted" data-read-back-summary>${esc(pairs.join(" · "))}</div>` : "";
}

// What the server read back, in one line, with key verified fields shown and full record a tap away.
/** @param {ReadBack|null|undefined} rb */
function readBackLine(rb) {
  if (!rb) return "";
  const cls = rb.matches === true ? "ok" : rb.matches === false ? "warn" : "";
  const said = rb.matches === true ? "Read back: it matches" : rb.matches === false ? "Read back: it does not match" : "Not read back";
  const summary = readBackSummary(rb.found);
  const found = rb.found && Object.keys(rb.found).length ? `<details class="small"><summary>What was found</summary><pre>${esc(JSON.stringify(rb.found, null, 2))}</pre></details>` : "";
  return `<div class="small" data-read-back="${esc(rb.matches === true ? "matches" : rb.matches === false ? "differs" : "none")}"><span class="chip ${cls}">${esc(said)}</span> <span class="muted">${esc(rb.note)}</span>${summary}${found}</div>`;
}

/** @param {Receipt} r */
function receiptRow(r) {
  const undo = r.reversible ? ` · <button type="button" class="link small" data-receipt-undo="${esc(r.call_id)}" data-receipt-what="${esc(r.what)}" data-receipt-tool="${esc(r.tool)}">Undo</button>` : "";
  const to = Array.isArray(r.to) && r.to.length ? `<div class="small muted receipt-to" data-receipt-to>To: ${r.to.map(esc).join(", ")}</div>` : "";
  return `<li data-receipt="${esc(r.call_id)}"><div>${esc(r.what)}</div>${to}<div class="small faint">${esc(r.tool)} · ${esc(when(r.at))}${undo}</div>${readBackLine(r.read_back)}</li>`;
}

/** Format receipts as a shareable verifiable Markdown proof block.
 * @param {string} title @param {Receipt[]} receipts */
function receiptsToMarkdown(title, receipts) {
  const lines = [`## Magnus Verification Proof — ${title || "Task"}`, ""];
  for (const r of receipts || []) {
    const rb = r.read_back?.matches === true ? "✓ Verified read-back" : r.read_back?.matches === false ? "⚠ Read-back mismatch" : "Logged";
    const to = Array.isArray(r.to) && r.to.length ? ` (To: ${r.to.join(", ")})` : "";
    lines.push(`- **${r.what}**${to} — \`${r.tool}\` at ${r.at || ""} · ${rb}`);
  }
  return lines.join("\n");
}

/** Format receipts as structured JSON for audit attachment.
 * @param {string} title @param {Receipt[]} receipts */
function receiptsToJson(title, receipts) {
  return JSON.stringify({ title: title || "Task", exported_at: new Date().toISOString(), receipts: receipts || [] }, null, 2);
}

/** @type {Record<string, { title: string, receipts: Receipt[] }>} */
const EvidenceCache = {};

if (typeof document !== "undefined" && typeof document.addEventListener === "function") {
  document.addEventListener("click", async e => {
    const copyBtn = e.target.closest?.("[data-evidence-copy-md]");
    if (copyBtn) {
      e.preventDefault();
      const entry = EvidenceCache[copyBtn.dataset.evidenceCopyMd];
      if (!entry) return;
      const md = receiptsToMarkdown(entry.title, entry.receipts);
      try { await navigator.clipboard?.writeText?.(md); if (typeof toast === "function") toast("Verification proof copied (Markdown)."); }
      catch { if (typeof toast === "function") toast("Copy failed."); }
      return;
    }
    const dlBtn = e.target.closest?.("[data-evidence-download-json]");
    if (dlBtn) {
      e.preventDefault();
      const entry = EvidenceCache[dlBtn.dataset.evidenceDownloadJson];
      if (!entry) return;
      const blob = new Blob([receiptsToJson(entry.title, entry.receipts)], { type: "application/json" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `magnus-receipts-${dlBtn.dataset.evidenceDownloadJson}.json`;
      document.body.append(a);
      a.click();
      setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
      return;
    }
    const b = e.target.closest?.("[data-receipt-undo]");
    if (!b || typeof Server === "undefined") return;
    e.preventDefault();
    const callId = b.dataset.receiptUndo, what = b.dataset.receiptWhat || b.dataset.receiptTool || "that action";
    const taskId = typeof Panel !== "undefined" ? Panel.task?.task_id : null;
    const thread = typeof Panel !== "undefined" ? Panel.task?.thread : null;
    b.disabled = true;
    try {
      if (taskId && Server.tasks?.undo) {
        const u = await Server.tasks.undo(taskId, callId);
        if (typeof toast === "function") toast("Undo started.");
        if (u?.task_id && typeof openPanel === "function") await openPanel(`task:${u.task_id}`);
      } else {
        const r = await Server.chat.send({ text: `Undo this action (${callId}): ${what}`, ...(thread ? { thread } : {}) });
        if (typeof toast === "function") toast("Started an undo request.");
        if (r?.thread && typeof location !== "undefined") location.hash = `#/chat/${encodeURIComponent(r.thread)}`;
      }
    } catch (err) {
      b.disabled = false;
      if (typeof toast === "function") toast(err.message);
    }
  });
}

// A running task's status line changes with every step it takes. Drawn into
// the card's HTML, each step changed the conversation's HTML and the whole
// thread was redrawn on every poll (0.2.6-test.109: the chat window kept
// refreshing). The card holds a fixed slot; the line is set in place.
/** @type {Record<string, string>} */
const StatusLines = {};
/** @param {{ task_id: string, status_line?: string|null }} t @param {string} cls */
function statusLineSlot(t, cls) {
  StatusLines[t.task_id] = t.status_line || "Working on it";
  return `<span class="${cls}" data-status-line="${esc(t.task_id)}">Working on it</span>`;
}
/** @param {ParentNode|null|undefined} root */
function liveStatusLines(root) {
  for (const el of root?.querySelectorAll?.("[data-status-line]") || []) {
    const line = StatusLines[el.dataset.statusLine];
    if (line && el.textContent !== line) el.textContent = line;
  }
}

// The result's evidence, after the deliverable: receipts with their
// read-backs, sources, and anything not done.
/** @param {{ task_id?: string, title?: string, evidence?: Evidence }} t */
function evidenceSection(t) {
  const e = t.evidence;
  if (!e) return "";
  const parts = [];
  if (e.receipts?.length) {
    const key = t.task_id || "task";
    EvidenceCache[key] = { title: t.title || "Task", receipts: e.receipts };
    parts.push(`<div class="spread" style="align-items:center;gap:8px;flex-wrap:wrap"><h3>What was done</h3><div class="row" style="gap:6px"><button type="button" class="small" data-evidence-copy-md="${esc(key)}">Copy proof</button><button type="button" class="small" data-evidence-download-json="${esc(key)}">Export JSON</button></div></div><ul class="receipts" data-receipts>${e.receipts.map(receiptRow).join("")}</ul>`);
  }
  if (e.sources?.length) {
    parts.push(`<h3>Sources</h3><ul class="sources" data-sources>${e.sources.map(s => {
      const where = [s.at, s.tool].filter(Boolean).join(" · ");
      return `<li><span>${esc(s.claim)}</span>${where ? ` <span class="small faint">${esc(where)}</span>` : ""}</li>`;
    }).join("")}</ul>`);
  }
  if (e.not_done?.length) {
    parts.push(`<h3>Not done</h3><ul class="not-done" data-not-done>${e.not_done.map(n => `<li><span class="chip warn">${esc(NOT_DONE_SAID[n.status] || n.status)}</span> ${esc(n.what)}${n.tool ? ` <span class="small faint">(${esc(n.tool)})</span>` : ""}</li>`).join("")}</ul>`);
  }
  return parts.join("");
}
