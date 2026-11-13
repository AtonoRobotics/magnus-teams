// The message controls both chats share (message-controls.js): which a
// message has, by who wrote it, who asked, what only its requester may see,
// and the viewer's role; how they are drawn; and that a chosen control runs
// its surface's adapter, for the personal chat and a team channel alike.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const MessageControls = createRequire(import.meta.url)(join(here, "../ui/message-controls.js"));
const { controls, html, run } = MessageControls;
const acts = list => list.map(c => (c.primary ? `*${c.act}` : c.act)).join(" ");

// Which controls, as a table: [what, message, caps, expected]. A `*` marks
// those at hand; the rest are in the menu, in this order.
const personal = { canReadAloud: true };
const team = { pins: true, canReadAloud: true };
const table = [
  ["personal: my message", { mine: true, deletes: true }, personal, "*edit copy branch share export save task delete"],
  ["personal: Magnus's answer", { agent: true, readable: true, retry: true, feedback: true, deletes: true }, personal, "*copy *listen retry branch share export save task feedback delete"],
  ["personal: Magnus's answer, being read", { agent: true, readable: true, retry: true, feedback: true, deletes: true }, { ...personal, reading: true }, "*copy *hush retry branch share export save task feedback delete"],
  ["personal: a task's result, nothing to read aloud", { agent: true, feedback: true, deletes: true }, { canReadAloud: false }, "*copy branch share export save task feedback delete"],
  ["team: my message", { mine: true, deletes: true }, team, "*edit copy branch share export save task pin delete"],
  ["team: someone else's message", {}, team, "*copy branch share export save task pin"],
  ["team: someone else's message, pinned, as an admin", { pinned: true, deletes: true }, team, "*copy branch share export save task pin delete"],
  ["team: the agent's answer to me", { agent: true, readable: true, retry: true, feedback: true, deletes: true }, team, "*copy *listen retry branch share export save task pin feedback delete"],
  ["team: the agent's answer to someone else", { agent: true, readable: true }, team, "*copy *listen branch share export save task pin"],
  ["team: an answer only I may see", { agent: true, readable: true, retry: true, feedback: true, private: true, deletes: true }, team, "*copy *listen retry branch export task pin feedback delete"],
  ["team: an answer only its requester may see, to me as another", { agent: true, readable: true, private: true }, team, "*copy *listen branch export task pin"],
  ["team: a guest, someone else's message", {}, { ...team, guest: true }, "*copy branch share export"],
  ["team: a guest's own message", { mine: true, deletes: true }, { ...team, guest: true }, "*edit copy branch share export delete"],
];
for (const [what, m, caps, want] of table) assert.equal(acts(controls(m, caps)), want, what);
const labels = Object.fromEntries(controls({ agent: true, pinned: true, readable: true, deletes: true, feedback: true, retry: true }, team).map(c => [c.act, c.label]));
assert.deepEqual(labels, { copy: "Copy response", listen: "Read aloud", retry: "Retry", branch: "Branch", share: "Share", export: "Export", save: "Save to Files", task: "Make a task", pin: "Unpin", feedback: "Feedback", delete: "Delete message" });
assert.equal(controls({}, team)[0].label, "Copy message", "a person's message is not a response");

// Drawn: what is at hand as icons, the rest in the menu, the surface's own
// controls first, and read-aloud tied to its entry.
const drawn = html("menu-1", controls({ agent: true, readable: true, deletes: true }, personal), { voiceEntry: "e1", lead: '<button data-reply></button>' });
assert.match(drawn, /^<div class="msg-actions small" role="group" aria-label="Message actions"><button data-reply><\/button><button class="icon" data-msg-act="copy"/);
assert.match(drawn, /data-msg-act="listen" data-voice-entry="e1"/);
assert.match(drawn, /<div id="menu-1" class="action-menu" role="menu" aria-label="More message actions" hidden><button role="menuitem" tabindex="-1" data-msg-act="branch">Branch<\/button>/);
assert.doesNotMatch(drawn.split('class="action-menu"')[1], /data-msg-act="copy"/, "a control at hand is in the menu too");
assert.match(MessageControls.shareRow("share-1", [{ id: "mo", name: "M<o" }]), /<select id="share-1" data-share-to><option value="mo">M&#60;o<\/option><\/select>[^]*data-msg-act="share-send"[^]*data-msg-act="share-cancel"/);

// Run: each action is its adapter's method, for either surface.
const fake = names => {
  const called = [];
  const adapter = Object.fromEntries(names.map(n => [n, function () { called.push(n); assert.equal(this, adapter); return n; }]));
  return { adapter, called };
};
const surfaces = {
  personal: ["copy", "edit", "editCancel", "editSend", "retry", "branch", "share", "shareCancel", "shareSend", "export", "save", "listen", "hush", "task", "delete"],
  team: ["copy", "edit", "retry", "branch", "share", "shareCancel", "shareSend", "export", "save", "listen", "hush", "task", "pin", "delete"],
};
for (const [surface, names] of Object.entries(surfaces)) {
  const offered = new Set(table.filter(([what]) => what.startsWith(surface)).flatMap(([, m, caps]) => controls(m, caps).map(c => c.act)).filter(a => a !== "feedback"));
  for (const act of [...offered, "share-send", "share-cancel"]) {
    const { adapter, called } = fake(names);
    assert.equal(await run(act, adapter), MessageControls.method(act), `${surface}: ${act} did not run its method`);
    assert.deepEqual(called, [MessageControls.method(act)], `${surface}: ${act}`);
  }
  const { adapter, called } = fake(names);
  assert.equal(await run("nothing-like-it", adapter), undefined);
  assert.deepEqual(called, [], `${surface}: an unknown action ran something`);
}
assert.equal(MessageControls.method("edit-send"), "editSend");

// Feedback: one dialog for both, its verdict and (personal) quick
// reactions, and focus back on the button it came from when it closes.
const body = { kids: [], append(el) { this.kids.push(el); } };
const focused = [];
const element = () => {
  const el = { attrs: {}, kids: [], setAttribute(k, v) { this.attrs[k] = v; }, append(c) { this.kids.push(c); }, remove() { body.kids = body.kids.filter(x => x !== el); },
    showModal() { el.open = true; }, close() { el.open = false; el.onclose?.(); } };
  return el;
};
globalThis.document = { createElement: element, body };
globalThis.verdictBar = (task, v) => `<div class="verdict" data-verdict-for="${task}">${v ? v.verdict : ""}</div>`;
const trigger = { focus: o => focused.push(o) };
const reacted = [];
const d = await run("feedback", { feedback: async () => ({ task: "t1", verdict: { verdict: "good" }, reaction: "great", react: async a => reacted.push(a), focus: () => trigger }) });
assert.ok(d.open && body.kids.includes(d), "the dialog did not open");
assert.match(d.innerHTML, /^<h2>Feedback<\/h2><div class="verdict" data-verdict-for="t1">good<\/div><div class="row"><button data-feedback-react="great" aria-pressed="true">/);
const target = sel => ({ closest: s => (s === sel ? { dataset: { feedbackReact: "annoying" } } : null) });
await d.onclick({ target: target("[data-feedback-react]") });
assert.deepEqual(reacted, ["annoying"]);
assert.ok(!d.open && !body.kids.includes(d), "a reaction did not close the dialog");
assert.deepEqual(focused, [{ preventScroll: true }], "focus did not go back to where the dialog was opened from");
const t = await run("feedback", { feedback: () => ({ task: "t2", focus: () => trigger }) });
assert.doesNotMatch(t.innerHTML, /data-feedback-react/, "the team's dialog offers the personal chat's reactions");
await t.onclick({ target: target("[data-close-feedback]") });
assert.equal(focused.length, 2, "closing the team's dialog lost focus");

// The team chat runs them through the server's routes for Share, Save to
// Files and Retry, which keep the workspace's rules.
const calls = [];
const sandbox = {
  UI: {}, route: () => {}, esc: s => String(s ?? ""), voice: { entry: null }, ICONS: new Proxy({}, { get: () => "" }), App: { canReadAloud: true }, enc: encodeURIComponent,
  $: () => ({ value: "dana" }), $$: () => [], paint: () => true, when: () => "", firstLine: s => String(s || "").split("\n")[0], icon: () => "", toast: () => {}, ask: async () => true,
  Magnus: {}, Compose: { markdown: s => s, maskedText: s => s }, CSS: { escape: s => s }, TeamPanel: { refresh: () => {} },
  api: async (m, p, b) => { calls.push([m, p, b]); return {}; },
  document: { addEventListener: () => {} }, location: { hash: "" }, setTimeout, clearTimeout, console,
};
sandbox.globalThis = sandbox;
const files = ["message-controls.js", "teams.js", "team-chat.js", "team-panel.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { Team, TeamChat };`, sandbox);
const { Team, TeamChat } = sandbox.__x;
Team.home = { me: "sam", role: "member", agent: { id: "agent", name: "Magnus" }, members: [{ id: "sam", name: "Sam" }, { id: "dana", name: "Dana" }], channels: [{ id: "c1" }], projects: [] };
TeamChat.s = { ws: "w", channel: "c1", view: { messages: [], names: {} }, pending: [], center: { isConnected: false } };
TeamChat.draw = () => {};
const answer = { id: "agent:t1", sender: "agent", requester: "sam", task: "t1", text: "done", at: "" };
for (const [act, route, body_] of [["save", "save", {}], ["share-send", "share", { person: "dana" }], ["retry", "retry", {}]]) {
  calls.length = 0;
  await TeamChat.msgAct(act, answer, { disabled: false });
  assert.deepEqual(JSON.parse(JSON.stringify(calls[0])), ["POST", `/v1/workspaces/w/channels/c1/messages/agent%3At1/${route}`, body_], `${act} did not go to the server`);
}
console.log("message controls ok");
