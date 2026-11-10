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

// The card. `canRespond` is whether this person may answer it.
function askCard(i, canRespond) {
  const a = i.ask || {};
  const d = i.details || {};
  const scope = { allow: a.allow_scope === "task" ? "approve:task" : "approve:once", allow_always: "approve:always", dont_allow: "reject" };
  const buttons = canRespond
    ? `<div class="row ask-answers">${(a.offers || []).map((o, n) => `<button class="${n === 0 ? "primary" : ""}" data-item="${esc(i.id)}" data-decision="${scope[o]}" data-offer="${esc(o)}">${esc(ASK_LABELS[o])}</button>`).join("")}</div>`
    : `<p class="small faint">Waiting for the person who can answer it.</p>`;
  return `<div class="ask" data-ask="${esc(i.id)}"${a.conversation ? ` data-conversation="${esc(a.conversation)}"` : ""}>
    <p class="ask-sentence" data-ask-sentence>${esc(a.sentence || i.summary || "")}</p>
    ${i.consequence ? `<p class="consequence small" role="note">${esc(i.consequence)}</p>` : ""}
    <details class="ask-content" data-ask-content><summary>See exactly what it would do</summary>${askContent(d)}</details>
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
