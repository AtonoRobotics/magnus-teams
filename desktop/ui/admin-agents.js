// Admin › Agents, and the kill switch's levels below the server (spec rev
// 141). Every agent on the server (the team agent and each person's) with
// what it runs now, who asked and what it has spent; Pause holds new work,
// Stop is that agent's kill switch and stops running work too. The top bar's
// kill switch gains one person's agents, one agent and one connector.
// Pressing needs no step-up; releasing and pausing go through adminApi.
"use strict";

/**
 * @typedef {{kind: string, person?: string, schedule?: string, workspace?: string}} AgentsAsker
 * @typedef {{run: string, task: string, kind: string, status: string, waiting_on: ?string, asked_by: AgentsAsker, started: string, spent_usd: number, actions: number, sub_agents: {run: string, depth: number, status: string, spent_usd: number}[]}} AgentsRun
 * @typedef {{by: string, name: string, at: string}} AgentsPulled
 * @typedef {{id: string, name: string, person: ?string, running: AgentsRun[], schedules: any[], spent_usd: number, paused: ?AgentsPulled, stopped: ?AgentsPulled, person_stopped: ?AgentsPulled}} AgentsRow
 * @typedef {{agents: AgentsRow[], connectors: string[]}} AgentsList
 */

/** Dollars as the Agents list shows them.
 * @param {number} n */
function agentsUsd(n) {
  return `$${(n || 0).toFixed(2)}`;
}

/** Who asked, in words, without anyone's content.
 * @param {AgentsAsker} a */
function agentsAsker(a) {
  if (!a) return "";
  if (a.kind === "schedule") return `a schedule of ${esc(a.person || "")}`;
  if (a.kind === "magnus") return "Magnus, in the background";
  if (a.kind === "member") return `${esc(a.person || "")} in a team thread`;
  if (a.kind === "sub_agent") return `a sub-agent, for ${esc(a.person || "")}`;
  return esc(a.person || "");
}

/** Ask the admin to pick one of `options` ([id, name]); null when cancelled.
 * @param {string} title @param {[string, string][]} options
 * @returns {Promise<?{target: string, name: string}>} */
function agentsPick(title, options) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask";
    d.setAttribute("aria-label", title);
    d.innerHTML = `<form class="stack" data-agents-pick>
      <h2>${esc(title)}</h2>
      ${options.length ? `<select name="target">${options.map(([id, name]) => `<option value="${esc(id)}">${esc(name)}</option>`).join("")}</select>` : `<p class="empty">There is nothing to stop.</p>`}
      <div class="row"><button type="button" data-agents-cancel>Cancel</button>${options.length ? `<button class="danger" type="submit">Stop</button>` : ""}</div>
    </form>`;
    let chosen = null;
    $("[data-agents-cancel]", d).onclick = () => d.close();
    $("form", d).onsubmit = e => {
      e.preventDefault();
      const target = String(new FormData(/** @type {HTMLFormElement} */ (e.target)).get("target") || "");
      chosen = { target, name: options.find(([id]) => id === target)?.[1] || target };
      d.close();
    };
    d.addEventListener("close", () => { d.remove(); done(chosen); });
    document.body.append(d);
    d.showModal();
  });
}

/** @returns {Promise<AgentsList>} */
function agentsList() {
  return api("GET", "/v1/admin/agents");
}

/** The switches of one level that are on, for the top bar's Release buttons.
 * @param {string} level */
async function agentsActive(level) {
  const v = await api("GET", "/v1/admin/kill-switches");
  return (v.on || []).filter(s => s.level === level);
}

registerKillLevel({
  level: "person", label: "Stop a person's agents",
  choose: async () => agentsPick("Stop one person's agents", (await agentsList()).agents.filter(a => a.person).map(a => [a.person || "", a.name])),
  active: () => agentsActive("person"),
});
registerKillLevel({
  level: "agent", label: "Stop an agent",
  choose: async () => agentsPick("Stop one agent", (await agentsList()).agents.map(a => [a.id, a.person ? `${a.name}'s agent` : a.name])),
  active: () => agentsActive("agent"),
});
registerKillLevel({
  level: "connector", label: "Stop a connector",
  choose: async () => agentsPick("Stop one connector, for every agent", (await agentsList()).connectors.map(c => [c, c])),
  active: () => agentsActive("connector"),
});

/** One running task, with its sub-agents.
 * @param {AgentsRun} r */
function agentsRunRow(r) {
  const subs = r.sub_agents.length ? ` · ${r.sub_agents.length} sub-agent${r.sub_agents.length === 1 ? "" : "s"} (${r.sub_agents.map(s => esc(s.status)).join(", ")})` : "";
  return `<li data-agents-run="${esc(r.run)}">${esc(r.kind)} · ${esc(r.status)}${r.waiting_on ? ` on ${esc(r.waiting_on)}` : ""} · asked by ${agentsAsker(r.asked_by)} · ${agentsUsd(r.spent_usd)}${subs}</li>`;
}

/** One agent's card: its state, what it runs, and its controls.
 * @param {AgentsRow} a */
function agentsCard(a) {
  const state = a.stopped ? `<span class="chip bad">Stopped by ${esc(a.stopped.name)} ${esc(when(a.stopped.at))}</span>`
    : a.person_stopped ? `<span class="chip bad">Stopped with ${esc(a.name)}'s agents by ${esc(a.person_stopped.name)}</span>`
    : a.paused ? `<span class="chip">Paused by ${esc(a.paused.name)} ${esc(when(a.paused.at))}</span>` : `<span class="chip">Working</span>`;
  const id = esc(a.id);
  const buttons = [
    a.paused ? `<button data-agents-resume="${id}">Resume</button>` : `<button data-agents-pause="${id}">Pause</button>`,
    a.stopped ? `<button data-agents-release="${id}">Release</button>` : `<button class="danger" data-agents-stop="${id}">Stop</button>`,
    a.person ? (a.person_stopped ? `<button data-agents-release-person="${esc(a.person)}">Release ${esc(a.name)}'s agents</button>` : `<button class="danger" data-agents-stop-person="${esc(a.person)}">Stop all of ${esc(a.name)}'s agents</button>`) : "",
    `<button data-agents-activity="${id}">What it did</button>`,
  ].join("");
  const scheds = a.schedules.length ? `<p class="small muted">${a.schedules.length} schedule${a.schedules.length === 1 ? "" : "s"}${a.schedules.some(s => s.paused) ? ", some paused" : ""}</p>` : "";
  return `<article class="card" data-agent="${id}">
    <header class="row"><h3>${esc(a.person ? `${a.name}'s agent` : a.name)}</h3>${state}<span class="small muted">spent ${agentsUsd(a.spent_usd)}</span></header>
    ${a.running.length ? `<ul class="small">${a.running.map(agentsRunRow).join("")}</ul>` : `<p class="small muted">Nothing running.</p>`}
    ${scheds}
    <div class="row">${buttons}</div>
    <div data-agents-done="${id}"></div>
  </article>`;
}

/** What an agent did, from the log: each run and each decision on a tool.
 * A person's agent's is their data, and they are told it was read.
 * @param {HTMLElement} el @param {string} agent */
async function agentsActivity(el, agent) {
  const v = await api("GET", `/v1/admin/agents/${encodeURIComponent(agent)}/activity`);
  const rows = (v.runs || []).map(r => `<li>${esc(when(r.started))} · ${esc(r.kind)} · ${esc(r.status)} · asked by ${agentsAsker(r.asked_by)} · ${agentsUsd(r.spent_usd)}${r.decisions.length ? `<ul>${r.decisions.map(d => `<li>${esc(d.tool)}: ${esc(d.outcome)} (${esc(d.stage)})</li>`).join("")}</ul>` : ""}</li>`);
  paint(el, `${agent.startsWith("person:") ? `<p class="small muted">This is their data: they have been told you read it.</p>` : ""}${rows.length ? `<ul class="small">${rows.join("")}</ul>` : `<p class="small muted">It has done nothing yet.</p>`}`);
}

/** Admin › Agents.
 * @param {HTMLElement} el */
async function agentsSection(el) {
  const v = await agentsList();
  paint(el, `<h2>Agents</h2>
    <p class="small muted">Pause holds new work and lets what runs finish. Stop stops running work too, and is that agent's kill switch.</p>
    <div class="stack" data-agents>${v.agents.map(agentsCard).join("")}</div>`);
  $("[data-agents]", el).onclick = async e => {
    const b = /** @type {HTMLElement} */ (e.target).closest("button");
    if (!b) return;
    const d = b.dataset;
    try {
      if (d.agentsActivity) { await agentsActivity(/** @type {HTMLElement} */ ($(`[data-agents-done="${CSS.escape(d.agentsActivity)}"]`, el)), d.agentsActivity); return; }
      if (d.agentsPause) await adminApi("POST", `/v1/admin/agents/${encodeURIComponent(d.agentsPause)}/pause`, {});
      if (d.agentsResume) await adminApi("POST", `/v1/admin/agents/${encodeURIComponent(d.agentsResume)}/resume`, {});
      if (d.agentsStop) await api("POST", killPath("agent", d.agentsStop, "press"), {});
      if (d.agentsRelease) await adminApi("POST", killPath("agent", d.agentsRelease, "release"), {});
      if (d.agentsStopPerson) await api("POST", killPath("person", d.agentsStopPerson, "press"), {});
      if (d.agentsReleasePerson) await adminApi("POST", killPath("person", d.agentsReleasePerson, "release"), {});
    } catch (err) { if (!err.cancelled) toast(err.message); }
    await adminPoll();
    await agentsSection(el);
  };
}

registerAdminSection("agents", agentsSection);
