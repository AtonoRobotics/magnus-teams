// Who is driving a computer, the agent's takeover request, and the pinned
// screen's panel, as screen.js draws them.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
const sandbox = {
  App: { me: { id: "alice" }, tasks: [{ task_id: "t1", title: "Book the flight" }], inbox: [{ id: "i1", kind: "takeover_request", can_respond: true }] },
  PANELS: {}, PANEL_TITLES: {}, Panel: { what: null },
  esc, enc: encodeURIComponent, firstLine: s => s.split("\n")[0], fieldValue: v => `<pre>${esc(v)}</pre>`,
  api: async (m, p) => { calls.push([m, p]); return { id: "c", status: "running", controller: null, driving: { by: "agent", person: null, runs: [{ run: "r1", task: "t1", status: "running" }], request: null } }; },
  document: { addEventListener: () => {} }, location: {}, $: () => null, setTimeout,
};
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/screen.js"), "utf8"), sandbox);

const running = driving => ({ id: "c", status: "running", controller: driving.person, driving });
const agent = sandbox.drivingLine(running({ by: "agent", person: null, runs: [{ run: "r1", task: "t1", status: "running" }], request: null }));
assert.match(agent, /data-driving="agent"/);
assert.match(agent, /Book the flight/, "the status does not say which task is using it");
assert.match(sandbox.drivingLine(running({ by: "person", person: "alice", runs: [], request: null })), /You have control\. The agent is paused\./);
assert.match(sandbox.drivingLine(running({ by: "person", person: "bob", runs: [], request: null })), /bob has control/);
assert.match(sandbox.drivingLine(running({ by: "no_one", person: null, runs: [], request: null })), /No one is using it/);
assert.equal(sandbox.drivingLine({ status: "hibernated", controller: null }), "", "an asleep computer claimed a driver");

const asked = running({ by: "no_one", person: null, runs: [], request: { item: "i1", reason: "captcha", why: "Solve the <b>puzzle</b>.", run: "r1", task: "t1", at: "" } });
const banner = sandbox.takeoverAsk(asked);
assert.match(banner, /The agent asks you to take over/);
assert.match(banner, /A CAPTCHA/);
assert.match(banner, /data-take-over="c"/, "the request cannot be accepted");
assert.match(banner, /data-decision="decline"/, "the request cannot be declined");
assert.doesNotMatch(banner, /<b>puzzle/, "the agent's words were not escaped");
assert.equal(sandbox.takeoverAsk(running({ by: "agent", person: null, runs: [], request: null })), "");

const card = sandbox.takeoverRequestCard({ id: "i1", can_respond: true, details: { computer: "c", reason: "passkey", why: "Touch your key." } });
for (const want of [/data-take-over="c"/, /data-open="computer:c"/, /data-decision="done"/, /data-decision="decline"/, /A passkey/]) assert.match(card, want);
assert.match(sandbox.takeoverRequestCard({ id: "i1", can_respond: false, details: {} }), /Waiting for its person/);

assert.equal(sandbox.PANEL_TITLES.computer, "Computer");
const { body } = await sandbox.PANELS.computer("c");
assert.deepEqual(calls[0], ["GET", "/v1/computers/c"]);
assert.match(body, /data-pinned-screen/, "the pinned panel shows no screen");
assert.match(body, /The agent is using it/);
console.log("screen ok");
