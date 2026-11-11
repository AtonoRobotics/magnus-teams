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
  document: { addEventListener: () => {} }, location: {}, $: () => null, setTimeout, UI: { computer: "<rect/>" },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/screen.js"), "utf8"), sandbox);

const running = driving => ({ id: "c", status: "running", controller: driving.person, driving });
const agent = sandbox.drivingLine(running({ by: "agent", person: null, runs: [{ run: "r1", task: "t1", status: "running" }], request: null }));
assert.match(agent, /data-driving="agent"/);
assert.match(agent, /Book the flight/, "the status does not say which task is using it");
assert.match(sandbox.drivingLine(running({ by: "person", person: "alice", runs: [], request: null })), /You have control\. Magnus is paused\./);
assert.match(sandbox.drivingLine(running({ by: "person", person: "bob", runs: [], request: null })), /bob has control/);
assert.match(sandbox.drivingLine(running({ by: "no_one", person: null, runs: [], request: null })), /No one is using it/);
assert.equal(sandbox.drivingLine({ status: "hibernated", controller: null }), "", "an asleep computer claimed a driver");

// Which agent is using it, and for whom (spec: Status): by name, never "the agent".
const forBob = running({ by: "agent", person: null, runs: [{ run: "r1", task: "t1", status: "running", agent: "Magnus", sub: false, for: "bob", for_name: "Bob" }], request: null });
assert.match(sandbox.drivingLine(forBob), /Magnus is using it.*for Bob on .*Book the flight/s);
assert.equal(sandbox.controlWords(forBob, false).pill, "Magnus is working");
assert.equal(sandbox.controlWords(forBob, false).text, "Magnus has control. It is working for Bob on Book the flight.");
const forMe = running({ by: "agent", person: null, runs: [{ run: "r1", task: "t1", status: "running", agent: "Magnus", sub: true, for: "alice", for_name: "Alice" }], request: null });
assert.match(sandbox.controlWords(forMe, false).text, /working for you on Book the flight \(with a sub-agent\)/);
assert.equal(sandbox.controlWords({ ...forBob, controller: "bob", controller_name: "Bob" }, false).text, "Bob has control. Magnus is paused.");

// The conversation's head says what the person's own computer is doing, and
// a click pins its screen beside the conversation.
const mine = { ...forMe, owner: { person: "alice" } };
assert.equal(sandbox.computerChip([]), "", "a person with no computer was shown one");
assert.equal(sandbox.computerChip([{ ...mine, owner: { workspace: "w" } }]), "", "a team's computer was shown as the person's own");
assert.match(sandbox.computerChip([mine]), /data-open="computer:c"/, "the chip does not pin the screen");
assert.match(sandbox.computerChip([mine]), /Magnus is using your computer/);
assert.match(sandbox.computerChip([{ ...mine, status: "hibernated", driving: { by: "no_one", runs: [], request: null } }]), /Your computer is asleep/);
assert.match(sandbox.computerChip([{ ...mine, controller: "alice" }]), /You are in control of your computer/);

const asked = running({ by: "no_one", person: null, runs: [], request: { item: "i1", reason: "captcha", why: "Solve the <b>puzzle</b>.", run: "r1", task: "t1", at: "" } });
const banner = sandbox.takeoverAsk(asked);
assert.match(banner, /Magnus asks you to take over/);
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
assert.match(body, /data-pinned="c"/, "the pinned panel shows no screen");
assert.deepEqual(calls[1], ["POST", "/v1/computers/c/wake"], "pinning a computer did not wake it");
// Its strip, state and screen are drawn by computerScreen, checked in computer-panel-check.mjs.
console.log("screen ok");
