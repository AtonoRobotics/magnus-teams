// Admin › Agents and the kill switch's levels below the server (spec rev
// 141): the top bar can stop one person's agents, one agent or one
// connector, with no step-up, and release each through it; the Agents
// section lists every agent with what it runs, who asked and what it spent,
// and pauses, stops and shows what an agent did. The banner names who
// pulled a level's switch.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const read = f => readFileSync(join(ui, f), "utf8");
const page = read("index.html"), admin = read("admin.js"), agents = read("admin-agents.js");
const fast = readFileSync(join(here, "../../.github/workflows/fast.yml"), "utf8");

const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.indexOf("admin-agents.js") > scripts.indexOf("admin.js"), "index.html loads admin-agents.js after admin.js");
assert.match(fast, /node desktop\/tests\/admin-agents-check\.mjs/, "the fast tier runs this check");

const list = {
  connectors: ["github", "mail"],
  agents: [
    { id: "team", name: "Team agent", person: null, spent_usd: 1.5, paused: null, stopped: null, person_stopped: null, schedules: [],
      running: [{ run: "rt", task: "tt", kind: "task", status: "running", waiting_on: null, asked_by: { kind: "member", person: "bob" }, started: "x", spent_usd: 0.25, actions: 1, sub_agents: [{ run: "rs", depth: 1, status: "running", spent_usd: 0.1 }] }] },
    { id: "person:alice", name: "Alice", person: "alice", spent_usd: 2, paused: { by: "admin:root", name: "Root", at: "t" }, stopped: null, person_stopped: null, schedules: [{ id: "s1", paused: false }], running: [] },
  ],
};

function harness() {
  const calls = [], toasts = [], els = new Map(), dialogs = [];
  const el = sel => {
    if (!els.has(sel)) els.set(sel, { html: "", hidden: false, dataset: {}, listeners: {}, addEventListener(t, f) { this.listeners[t] = f; }, remove() { els.delete(sel); }, setAttribute() {} });
    return els.get(sel);
  };
  const ctx = {
    calls, toasts, els, dialogs, console, CSS: { escape: s => s },
    location: { hash: "#/admin/agents" },
    $: sel => el(sel),
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    paint: (box, html) => { box.html = html; },
    toast: m => toasts.push(m),
    when: () => "2 min ago",
    icon: k => `<svg data-icon="${k}"></svg>`,
    route: () => {},
    sidebar: () => {},
    document: { createElement: () => el("#stop-banner"), body: { append() {} } },
    setInterval: () => 1, clearInterval: () => {},
  };
  ctx.api = async (m, p, b) => {
    calls.push([m, p, b]);
    if (p === "/v1/admin/agents") return list;
    if (p === "/v1/admin/kill-switches") return { on: [{ level: "connector", target: "mail", by: "admin:root", by_name: "Root", at: "t" }], paused: [] };
    if (p === "/v1/stops") return { stops: [{ level: "agent", target: "team", by: "admin:root", by_name: "Root", at: "t" }] };
    if (p.endsWith("/activity")) return { runs: [{ started: "t", kind: "task", status: "completed", asked_by: { kind: "person", person: "alice" }, spent_usd: 0.5, decisions: [{ tool: "mail__send", outcome: "allow", stage: "policy" }] }] };
    return { ok: true };
  };
  ctx.Magnus = { api: ctx.api };
  vm.createContext(ctx);
  vm.runInContext(admin, ctx);
  vm.runInContext(agents, ctx);
  vm.runInContext("stepUp = async () => true", ctx);
  // Each picker answers with its first choice.
  vm.runInContext("agentsPick = async (title, options) => { globalThis.dialogs.push([title, options]); return options.length ? { target: options[0][0], name: options[0][1] } : null; }", ctx);
  return ctx;
}

// The three levels are in the top bar, beside the server's switch.
{
  const a = harness();
  const levels = vm.runInContext("AdminState.levels.map(l => l.level)", a);
  assert.deepEqual([...levels], ["person", "agent", "connector"]);
  await vm.runInContext("adminPoll()", a);
  await vm.runInContext("drawKillControl()", a);
  const bar = a.els.get("[data-admin-kill]").html;
  assert.match(bar, /data-kill-press="person">Stop a person&#39;s agents/);
  assert.match(bar, /data-kill-press="agent">Stop an agent/);
  assert.match(bar, /data-kill-press="connector">Stop a connector/);
  assert.match(bar, /data-kill-release="connector" data-kill-target="mail"/, "a connector's switch that is on can be released from the top bar");
  // The banner names who pulled a level's switch.
  assert.match(a.els.get("#stop-banner").html, /Root stopped an agent \(team\)/);
}

// Pressing a level: the admin picks what to stop; no step-up is asked.
for (const [level, target] of [["person", "alice"], ["agent", "team"], ["connector", "github"]]) {
  const a = harness();
  vm.runInContext("stepUp = async () => { throw new Error('a press asked for a step-up'); }", a);
  const press = { closest: s => (s === "[data-kill-press]" ? { dataset: { killPress: level } } : null) };
  await vm.runInContext("onKillClick", a)({ target: press });
  assert.ok(a.calls.some(([m, p]) => m === "POST" && p === `/v1/admin/kill-switch/${level}/${encodeURIComponent(target)}/press`), `${level}: ${JSON.stringify(a.calls)}`);
  assert.equal(a.toasts.length, 1, a.toasts.join());
}

// The Agents section: every agent, what it runs, who asked, spend, and its controls.
{
  const a = harness();
  const section = { html: "" };
  await vm.runInContext("agentsSection", a)(section);
  assert.match(section.html, /Team agent/);
  assert.match(section.html, /Alice&#39;s agent/);
  assert.match(section.html, /asked by bob in a team thread · \$0\.25 · 1 sub-agent \(running\)/, "a running task shows who asked, its spend and its sub-agents");
  assert.match(section.html, /Paused by Root/);
  assert.match(section.html, /data-agents-resume="person:alice"/, "a paused agent is resumed, not paused again");
  assert.match(section.html, /data-agents-stop="team"/);
  assert.match(section.html, /data-agents-stop-person="alice">Stop all of Alice's agents/);
  assert.match(section.html, /1 schedule/);
  const click = async dataset => {
    a.calls.length = 0;
    await a.els.get("[data-agents]").onclick({ target: { closest: () => ({ dataset }) } });
  };
  await click({ agentsStop: "team" });
  assert.ok(a.calls.some(([m, p]) => m === "POST" && p === "/v1/admin/kill-switch/agent/team/press"), "Stop is the agent's kill switch");
  await click({ agentsStopPerson: "alice" });
  assert.ok(a.calls.some(([m, p]) => m === "POST" && p === "/v1/admin/kill-switch/person/alice/press"));
  await click({ agentsPause: "team" });
  assert.ok(a.calls.some(([m, p]) => m === "POST" && p === "/v1/admin/agents/team/pause"));
  await click({ agentsRelease: "person:alice" });
  assert.ok(a.calls.some(([m, p]) => m === "POST" && p === "/v1/admin/kill-switch/agent/person%3Aalice/release"));
  await click({ agentsActivity: "person:alice" });
  assert.ok(a.calls.some(([m, p]) => m === "GET" && p === "/v1/admin/agents/person%3Aalice/activity"));
  const done = a.els.get('[data-agents-done="person:alice"]').html;
  assert.match(done, /they have been told you read it/, "reading a person's agent's activity does not say they are told");
  assert.match(done, /mail__send: allow \(policy\)/);
}

console.log("admin agents check passed");
