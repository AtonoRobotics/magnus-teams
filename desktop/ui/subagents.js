// The sub-agents a chat turn started, in the conversation that started them
// (spec, Chat and tasks; decisions/2026-11-09-chat-does-the-work.md): what
// each is doing, its status, what it waits on and its result. Nested ones
// sit under their parent.

/**
 * @typedef {{ run: string, parent: string, depth: number, task_id: string, goal: string,
 *   status: "running"|"suspended"|"completed"|"failed"|"cancelled", waiting_on: string|null,
 *   waiting_item: string|null, summary: string|null, tools: string[], at: string, ended_at: string|null }} SubAgentView
 */
/** @typedef {{ id: string, kind: "sub_agents", task_id: string, from_run: string, agents: SubAgentView[], at: string }} SubAgentsEntry */

/** A sub-agent's status in words, and the chip's class. */
const SUB_AGENT_STATUS = {
  running: ["run", "Working"], suspended: ["warn", "Waiting"], completed: ["ok", "Done"], failed: ["bad", "Failed"], cancelled: ["", "Stopped"],
};

/**
 * One sub-agent: its goal, its status, why it waits, and its result once it has one.
 * @param {SubAgentView} a
 */
function subAgentRow(a) {
  const [cls, label] = SUB_AGENT_STATUS[a.status] || ["", a.status];
  const waits = a.status === "suspended" && a.waiting_on ? `<span class="sub why">${esc((typeof WAITING === "object" && WAITING[a.waiting_on]) || "waiting")}</span>` : "";
  const ended = a.status !== "running" && a.status !== "suspended";
  const result = ended && a.summary
    ? `<details data-key="sub-agent:${esc(a.run)}"><summary class="small">${a.status === "completed" ? "Its result" : "Why it stopped"}</summary><div class="pre">${esc(a.summary)}</div></details>`
    : "";
  return `<li class="sub-agent" data-sub-agent="${esc(a.run)}" data-status="${esc(a.status)}" data-depth="${Math.min(Math.max(Number(a.depth) || 1, 1), 3)}">
    <div class="rg-main"><span class="ttl">${esc(firstLine(a.goal || "") || "A sub-agent")}</span>${waits}</div><span class="chip ${cls}">${esc(label)}</span>${result}</li>`;
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
