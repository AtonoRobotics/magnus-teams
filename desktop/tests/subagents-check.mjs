// Sub-agents a chat turn starts show in its conversation: what each is
// doing, its status, what it waits on and its result, as app.js and
// subagents.js draw a thread's entries.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const sandbox = {
  esc, firstLine: s => String(s).split("\n")[0],
  WAITING: { approval: "waiting for an approval" },
  waitingCard: id => id === "i1" ? `<div class="confirm" data-item="i1">Approve the send</div>` : "",
};
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("subagents.js"), sandbox);

/** @type {any} */
const entry = {
  id: "s:turn", kind: "sub_agents", task_id: "turn", from_run: "rt", at: "2026-11-09T10:00:00Z",
  agents: [
    { run: "r1", parent: "rt", depth: 1, task_id: "c1", goal: "Compare <b>flights</b> to Lisbon\nunder $600", status: "running", waiting_on: null, waiting_item: null, summary: null, tools: [], at: "", ended_at: null },
    { run: "r2", parent: "r1", depth: 2, task_id: "c2", goal: "Check TAP fares", status: "completed", waiting_on: null, waiting_item: null, summary: "Found three fares under $600.", tools: ["web_fetch"], at: "", ended_at: "" },
    { run: "r3", parent: "rt", depth: 1, task_id: "c3", goal: "Email the itinerary", status: "suspended", waiting_on: "approval", waiting_item: "i1", summary: null, tools: [], at: "", ended_at: null },
    { run: "r4", parent: "rt", depth: 1, task_id: "c4", goal: "Book the hotel", status: "failed", waiting_on: null, waiting_item: null, summary: "The site refused the card.", tools: [], at: "", ended_at: "" },
  ],
};
const card = sandbox.subAgentsCard(entry, {});
assert.match(card, /data-sub-agents="rt"/, "the card does not name the turn it belongs to");
assert.match(card, /4 sub-agents on this request/);
assert.match(card, /1 working · 1 of 4 done/, "the card does not count what is working and done");
for (const run of ["r1", "r2", "r3", "r4"]) assert.match(card, new RegExp(`data-sub-agent="${run}"`), `${run} is not shown`);
assert.match(card, /Compare &#60;b&#62;flights&#60;\/b&#62; to Lisbon/, "a goal was not shown, or not escaped");
assert.doesNotMatch(card, /under \$600<\/span>/, "more than the goal's first line was shown as its title");
assert.match(card, /data-sub-agent="r2"[^>]*data-depth="2"/, "a nested sub-agent is not set under its parent");
assert.match(card, /data-sub-agent="r1" data-status="running"[\s\S]*?Working/, "a working sub-agent does not say so");
assert.match(card, /Its result[\s\S]*Found three fares under \$600\./, "a finished sub-agent's result is not shown");
assert.match(card, /waiting for an approval/, "a waiting sub-agent does not say what it waits on");
assert.match(card, /data-item="i1"/, "the decision a sub-agent waits on is not in the conversation");
assert.match(card, /Why it stopped[\s\S]*The site refused the card\./, "a failed sub-agent does not say why");
assert.match(sandbox.subAgentsCard({ ...entry, agents: [entry.agents[1]] }), /A sub-agent on this request/);

// app.js draws the entry with the card; the personal chat (personal-chat.js)
// waits on what the sub-agents wait on and offers no message actions for
// it; and the page loads subagents.js.
const app = ui("app.js"), personal = ui("personal-chat.js");
assert.match(app, /case "sub_agents": return subAgentsCard\(e, ui\);/, "the conversation does not draw sub-agents");
assert.match(personal, /e\.kind === "sub_agents" \? e\.agents\.map\(a => a\.waiting_item\)/, "a sub-agent's decision is not read before the thread is drawn");
assert.match(personal, /if \(!e\.id \|\| e\.kind === "sub_agents"\) return null;/, "the sub-agents card offers message actions");
assert.match(ui("index.html"), /<script src="subagents\.js"><\/script>/, "the page does not load subagents.js");
console.log("sub-agents ok");
