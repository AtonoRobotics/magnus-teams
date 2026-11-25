// An ask as the person sees it (spec rev 140, How an ask looks): one plain
// sentence the server wrote from the action's fields, the exact content one
// tap away, and Allow, Allow always and Don't allow. Nothing else: the
// rule, the policy, the taint and the reviewer's reasoning are for the
// developer view (askReasoning below, drawn by dev.js).
"use strict";

const ASK_LABELS = { allow: "Allow", allow_always: "Allow always", dont_allow: "Don't allow" };


// One field of the exact content, every part of it, nothing shortened.
function askValue(v) {
  if (v === null || v === undefined) return `<span class="faint">none</span>`;
  if (Array.isArray(v)) {
    if (!v.length) return `<span class="faint">none</span>`;
    if (v.every(x => x !== null && x !== undefined && typeof x !== "object")) {
      return `<span>${v.map(x => esc(String(x))).join(", ")}</span>`;
    }
  }
  if (v && typeof v === "object" && !Array.isArray(v)) {
    const entries = Object.entries(v).filter(([, x]) => x !== null && x !== undefined && typeof x !== "object");
    if (entries.length && entries.length === Object.keys(v).length) {
      return `<div class="ask-kv">${entries.map(([k, val]) => `<div><span class="muted">${esc(k.replace(/_/g, " "))}:</span> <span>${esc(String(val))}</span></div>`).join("")}</div>`;
    }
  }
  return `<pre>${esc(typeof v === "string" ? v : typeof v === "object" ? JSON.stringify(v, null, 2) : String(v))}</pre>`;
}

// What the action carries and where it goes: the recipients and content
// the server read, then each field of the call.
function askContent(d) {
  const label = k => esc(k[0].toUpperCase() + k.slice(1));
  const ds = d.destinations || {};
  const to = Object.entries(ds.recipients || {}).filter(([, a]) => a.length).map(([k, a]) => `<div class="field"><span>${label(k)}</span><span>${a.map(esc).join(", ")}</span></div>`);
  const content = Object.entries(ds.content || {}).filter(([, v]) => v !== null && v !== "").map(([k, v]) => `<div class="field"><span>${label(k)}</span>${askValue(v)}</div>`);
  const input = d.input && typeof d.input === "object" && !Array.isArray(d.input) ? d.input : { input: d.input };
  const fields = Object.entries(input).map(([k, v]) => `<div class="field"><span>${esc(k)}</span>${askValue(v)}</div>`);
  return `<div class="what">${to.concat(content, fields).join("")}</div>`;
}

const ASK_TIERS = { read: "Read-only", sandbox: "Sandbox", reversible_write: "Reversible write", irreversible: "Irreversible", financial: "Financial" };

function askTierBadge(i) {
  const label = i?.tier ? ASK_TIERS[i.tier] : "";
  return label ? `<span class="ask-tier" data-tier="${esc(i.tier)}">${esc(label)}</span>` : "";
}

const ASK_PREVIEW_BODY_RE = /^(body|message|text|content|note|description|html|draft|query|command)$/i;
const ASK_PREVIEW_MAX_FIELDS = 5;
const ASK_PREVIEW_BODY_MAX = 320;

/**
 * Extracts concise, human-facing fields (recipients, subject and a body
 * excerpt) from what the gate read (`details.destinations`), or where it
 * read none, where the server says the action goes (`ask.destinations`).
 * Never the model's own input: the person allows what goes, not what the
 * model wrote. Never reads policy, rule, taint, or reviewer reasoning.
 * @param {any} i
 * @returns {{ rows: Array<[string, string]>, body: string }}
 */
function askSummaryFields(i) {
  const d = (i && i.details) || {};
  const ds = d.destinations || {};
  const rows = [];
  const seen = new Set();
  let body = "";
  const pushRow = (key, rawVal) => {
    if (rows.length >= ASK_PREVIEW_MAX_FIELDS) return;
    const val = Array.isArray(rawVal)
      ? rawVal.filter(x => x !== null && x !== undefined && x !== "").map(String).join(", ")
      : (rawVal !== null && rawVal !== undefined && typeof rawVal !== "object" ? String(rawVal).trim() : "");
    if (!val || val === "none") return;
    const lower = key.toLowerCase();
    if (seen.has(lower)) return;
    seen.add(lower);
    const nice = key.replace(/_/g, " ");
    rows.push([nice[0].toUpperCase() + nice.slice(1), val]);
  };
  for (const [k, a] of Object.entries(ds.recipients || {})) if (Array.isArray(a) && a.length) pushRow(k, a);
  if (!rows.length && Array.isArray(i?.ask?.destinations) && i.ask.destinations.length) {
    pushRow("Destination", i.ask.destinations);
  }
  for (const [k, v] of Object.entries(ds.content || {})) {
    if (v === null || v === undefined || v === "") continue;
    const str = typeof v === "string" ? v.trim() : typeof v === "object" ? "" : String(v);
    if (!str) continue;
    if (ASK_PREVIEW_BODY_RE.test(k)) { if (!body) body = str; }
    else pushRow(k, str);
  }
  if (body.length > ASK_PREVIEW_BODY_MAX) body = `${body.slice(0, ASK_PREVIEW_BODY_MAX).trimEnd()}…`;
  return { rows, body };
}

function askPreview(i) {
  const { rows, body } = askSummaryFields(i);
  if (!rows.length && !body) return "";
  const rowsHtml = rows.length
    ? `<div class="ask-preview-rows">${rows.map(([k, v]) => `<div class="ask-preview-row"><span class="ask-preview-key">${esc(k)}</span><span class="ask-preview-val">${esc(v)}</span></div>`).join("")}</div>`
    : "";
  const bodyHtml = body ? `<p class="ask-preview-body">${esc(body)}</p>` : "";
  return `<div class="ask-preview" data-ask-preview>${rowsHtml}${bodyHtml}</div>`;
}

const ASK_EDITABLE_KEY_RE = /^(to|cc|bcc|subject|title|body|message|text|content|note|description|summary|channel)$/i;

function askEditFields(i, d) {
  const inp = d.input && typeof d.input === "object" && !Array.isArray(d.input) ? d.input : {};
  const editable = Object.entries(inp).filter(([k, v]) => ASK_EDITABLE_KEY_RE.test(k) && typeof v === "string");
  if (!editable.length) return "";
  const controls = editable.map(([k, v]) => {
    const label = esc(k.replace(/_/g, " "));
    const multiline = ASK_PREVIEW_BODY_RE.test(k) || v.length > 80 || v.includes("\n");
    return multiline
      ? `<label class="small">${label}<textarea data-edit-field="${esc(k)}" data-orig="${esc(v)}" rows="3">${esc(v)}</textarea></label>`
      : `<label class="small">${label}<input type="text" data-edit-field="${esc(k)}" data-orig="${esc(v)}" value="${esc(v)}"></label>`;
  }).join("");
  return `<details class="ask-edit" data-ask-edit="${esc(i.id)}"><summary class="small">Edit before allowing</summary><div class="stack" style="margin-top:6px">${controls}</div></details>`;
}

// The card. `canRespond` is whether this person may answer it.
function askCard(i, canRespond) {
  const a = i.ask || {};
  const d = i.details || {};
  const scope = { allow: a.allow_scope === "task" ? "approve:task" : "approve:once", allow_always: "approve:always", dont_allow: "reject" };
  const editBlock = canRespond ? askEditFields(i, d) : "";
  const declineNote = canRespond ? `<input type="text" class="ask-decline-note" data-draft="rev-${esc(i.id)}" placeholder="Optional note if declining (tells Magnus what to change)…" aria-label="Optional note">` : "";
  const buttons = canRespond
    ? `<div class="row ask-answers">${(a.offers || []).map((o, n) => `<button class="${n === 0 ? "primary" : ""}" data-item="${esc(i.id)}" data-decision="${scope[o]}" data-offer="${esc(o)}">${esc(ASK_LABELS[o])}</button>`).join("")}</div>`
    : `<p class="small faint">Waiting for the person who can answer it.</p>`;
  return `<div class="ask" data-ask="${esc(i.id)}"${a.conversation ? ` data-conversation="${esc(a.conversation)}"` : ""}>
    ${askTierBadge(i)}
    <p class="ask-sentence" data-ask-sentence>${esc(a.sentence || i.summary || "")}</p>
    ${i.consequence ? `<p class="consequence small" role="note">${esc(i.consequence)}</p>` : ""}
    ${askPreview(i)}
    <details class="ask-content" data-ask-content><summary>See exactly what it would do</summary>${askContent(d)}</details>
    ${editBlock}
    ${declineNote}
    ${buttons}</div>`;
}

// A decided ask, in the person's words.
function askResolved(i) {
  const r = i.resolution || {};
  const said = { approve: "Allowed", reject: "Not allowed", expired: "No one answered in time" }[r.decision] || r.decision || "";
  const scope = { task: " for the rest of this work", always: " always" }[r.scope] || "";
  return `<p class="ask-sentence">${esc((i.ask && i.ask.sentence) || i.summary || "")}</p><p class="small muted">${esc(said + scope)}</p>`;
}

// For the developer view and the run inspector: why an ask asked.
const ASK_REASONS = { outside_request: "Auto Review judged it outside what was asked", payment: "A payment", new_destination: "Private data to a new destination after untrusted input", ask_first: "The person's ask-first rule" };
function askReasoning(x) {
  if (!x) return "";
  const t = x.taint || {};
  const taint = [t.untrusted ? "untrusted input" : "", t.private ? "private data" : ""].filter(Boolean).join(" and ") || "none";
  return `<div class="reasoning small" data-reasoning>
    <div><span class="muted">Ask:</span> ${esc(ASK_REASONS[x.reason] || x.reason || "before rev 140")}</div>
    <div><span class="muted">Rule:</span> <span class="mono">${esc(x.rule || "none")}</span></div>
    <div><span class="muted">Reason given:</span> ${esc(x.why || "")}</div>
    <div><span class="muted">Taint:</span> ${esc(taint)}</div></div>`;
}

// A boundary decision with its stage, outcome, reason and rule, in full.
function decisionRows(ds) {
  return (ds || []).map(d => `<div class="small" data-decision-row><span class="mono">${esc(d.stage)}: ${esc(d.outcome)}</span>${d.reason ? ` · ${esc(d.reason)}` : ""}${d.rule ? ` <span class="faint mono">(${esc(d.rule)})</span>` : ""}</div>`).join("");
}

globalThis.askCard = askCard;
globalThis.askResolved = askResolved;
globalThis.askReasoning = askReasoning;
globalThis.decisionRows = decisionRows;
globalThis.askSummaryFields = askSummaryFields;
globalThis.askTierBadge = askTierBadge;
