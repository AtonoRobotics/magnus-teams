// The sub-agents a chat turn started, in the conversation that started them
// (spec, Chat and tasks; decisions/2026-11-09-chat-does-the-work.md): what
// each is doing, its status, what it waits on and its result. Nested ones
// sit under their parent.

/**
 * @typedef {{ run: string, parent: string, depth: number, task_id: string, goal: string,
 *   status: "running"|"suspended"|"completed"|"failed"|"cancelled", waiting_on: string|null,
 *   waiting_item: string|null, status_line?: string|null, summary: string|null,
 *   outcome_view?: any, outputs?: Array<{ id: string, name: string, media_type?: string|null, size?: number|null }>,
 *   tools: string[], at: string, ended_at: string|null }} SubAgentView
 */
/** @typedef {{ id: string, kind: "sub_agents", task_id: string, from_run: string, agents: SubAgentView[], at: string }} SubAgentsEntry */

/** A sub-agent's status in words, and the chip's class. */
const SUB_AGENT_STATUS = {
  running: ["run", "Working"], suspended: ["warn", "Waiting"], completed: ["ok", "Done"], failed: ["bad", "Failed"], cancelled: ["", "Stopped"],
};

/** Extract readable findings text from a sub-agent's outcome_view blocks. @param {any} view */
function subAgentOutcomeText(view) {
  if (!Array.isArray(view) || !view.length) return "";
  const lines = [];
  for (const b of view) {
    if (!b || typeof b !== "object") continue;
    if (b.type === "text" && typeof b.text === "string" && b.text.trim()) lines.push(b.text.trim());
    else if (b.type === "stat" && (b.label || b.value !== undefined)) lines.push(`${b.label ? `${b.label}: ` : ""}${b.value ?? ""}`);
    else if (b.type === "table" && Array.isArray(b.rows)) {
      const hdr = Array.isArray(b.headers) ? b.headers.join(" | ") : "";
      if (hdr) lines.push(hdr);
      for (const row of b.rows.slice(0, 6)) if (Array.isArray(row)) lines.push(row.join(" | "));
      if (b.rows.length > 6) lines.push(`… (${b.rows.length} rows — click to inspect full table)`);
    }
  }
  return lines.join("\n").trim();
}

/**
 * One sub-agent: its goal, live status, why it waits, delivered files, and its
 * findings once finished. Clicking opens the sub-agent's task view.
 * @param {SubAgentView} a
 */
function subAgentRow(a) {
  const [cls, label] = SUB_AGENT_STATUS[a.status] || ["", a.status];
  const ended = a.status !== "running" && a.status !== "suspended";
  const sub = a.status === "suspended" && a.waiting_on
    ? `<span class="sub why">${esc((typeof WAITING === "object" && WAITING[a.waiting_on]) || "waiting")}</span>`
    : a.status === "running"
      ? (typeof statusLineSlot === "function" && a.task_id
        ? statusLineSlot({ task_id: a.task_id, status_line: a.status_line }, "sub why")
        : a.status_line ? `<span class="sub why">${esc(a.status_line)}</span>` : "")
      : (typeof when === "function" && (a.ended_at || a.at) ? `<span class="sub">${esc(when(a.ended_at || a.at))}</span>` : "");
  const titleHtml = `<span class="ttl">${esc(firstLine(a.goal || "") || "A sub-agent")}</span>${sub}`;
  const main = a.task_id
    ? `<a class="rg-main sub-agent-link" href="#/task/${typeof enc === "function" ? enc(a.task_id) : encodeURIComponent(a.task_id)}" data-open="task:${esc(a.task_id)}">${titleHtml}</a>`
    : `<div class="rg-main">${titleHtml}</div>`;
  const findings = ended ? (subAgentOutcomeText(a.outcome_view) || a.summary || "") : "";
  const result = ended && findings
    ? `<details open data-key="sub-agent:${esc(a.run)}"><summary class="small">${a.status === "completed" ? "Its result" : "Why it stopped"}</summary><div class="pre">${esc(findings)}</div></details>`
    : "";
  const outputs = ended && Array.isArray(a.outputs) && a.outputs.length
    ? `<div class="sub-agent-files">${a.outputs.map(f => `<a class="fchip" href="#/file/${typeof enc === "function" ? enc(f.id) : encodeURIComponent(f.id)}" data-open="file:${esc(f.id)}">${esc(f.name)}</a>`).join("")}</div>`
    : "";
  return `<li class="sub-agent" data-sub-agent="${esc(a.run)}" data-status="${esc(a.status)}" data-depth="${Math.min(Math.max(Number(a.depth) || 1, 1), 3)}">
    ${main}<span class="chip ${cls}">${esc(label)}</span>${result}${outputs}</li>`;
}

/**
 * The sub-agents one message's reply started, as one card; a decision one
 * of them waits on follows it.
 * @param {SubAgentsEntry} e
 * @param {{ composed?: any[] }} [ui]
 */
function subAgentsCard(e, ui = {}) {
  const agents = e.agents || [];
  const done = agents.filter(a => a.status === "completed").length;
  const working = agents.filter(a => a.status === "running").length;
  const head = agents.length === 1 ? "A sub-agent on this request" : `${agents.length} sub-agents on this request`;
  const count = working ? `${working} working · ${done} of ${agents.length} done` : `${done} of ${agents.length} done`;
  const waiting = typeof waitingCard === "function" ? agents.map(a => a.waiting_item ? waitingCard(a.waiting_item, ui) : "").filter(Boolean) : [];
  return `<div class="reqgroup chatcard sub-agents" data-sub-agents="${esc(e.from_run)}"><div class="rg-head"><span>${head}</span><span class="small muted num">${count}</span></div>
    <ul class="rg-rows">${agents.map(subAgentRow).join("")}</ul></div>${waiting.map(w => `<div class="decide">${w}</div>`).join("")}`;
}
