// One computer, always there (spec rev 142). The person's agent has exactly
// one computer and each workspace's team agent has one: the Computers page
// lists them with nothing to make or pick, a computer's page has no Delete,
// and its Reset keeps the files unless the person says otherwise.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

const idle = { by: "no_one", person: null, runs: [], request: null };
const mine = { id: "c", name: "Your computer", owner: { person: "alice" }, status: "hibernated", controller: null, driving: idle, waking: null };
const team = { id: "t", name: "Ops team computer", owner: { workspace: "w" }, status: "running", controller: null, driving: idle, waking: null };
let listed = [mine, team];
const { page, calls, sandbox } = load({
  hash: "#/computer",
  reply: (method, path) => {
    if (path === "/v1/computers") return { computers: listed };
    if (path === "/v1/local/computers") return [];
    if (path === "/v1/computers/c") return mine;
    if (path.endsWith("/reset")) return { id: "c2", replaces: "c", revoked: 0 };
    return {};
  },
});
sandbox.paint = (el, html) => { el.innerHTML = html; };
sandbox.Magnus.localState = async () => null;

// The list: the person's own computer and the team's, nothing to make.
await sandbox.showComputers(page.body);
await settle();
const html = page.body.innerHTML;
assert.doesNotMatch(html, /New computer|data-act="new-computer"/, "the Computers page offers to make a computer");
assert.match(html, /<h2>Your computer<\/h2>/, "the person's computer is not shown as theirs");
assert.match(html, /data-computer="c"/);
assert.match(html, /<h2>Team computers<\/h2>[\s\S]*data-computer="t"/, "the team's computer is not shown apart");
assert.ok(!calls.some(c => c === "POST /v1/computers"), "opening the page made a computer");

// Before the server has made it: it is coming, with nothing to press.
listed = [];
page.fresh();
await sandbox.showComputers(page.body);
await settle();
assert.match(page.body.innerHTML, /being set up/, "a person without their computer yet is not told it is coming");
assert.doesNotMatch(page.body.innerHTML, /New computer|You have no cloud computer/);

// One computer's page: Reset, and no way to end with none.
page.fresh();
sandbox.location.hash = "#/computer/c/screen";
await sandbox.showComputer(page.body, "c", "screen");
await settle();
const head = page.body.innerHTML;
assert.match(head, /data-act="reset"/, "the computer cannot be reset");
assert.doesNotMatch(head, /data-act="delete"|Delete computer/, "the computer can still be deleted");

// The reset asks which way, with keeping the files the default.
let dialog;
sandbox.document.createElement = () => {
  dialog = { className: "", innerHTML: "", listeners: {}, addEventListener(t, f) { this.listeners[t] = f; }, showModal() {}, remove() {}, close(v) { this.returnValue = v; this.listeners.close(); } };
  return dialog;
};
sandbox.document.body = { append() {} };
let focused = null;
const $ = sandbox.$;
sandbox.$ = (sel, root) => (root === dialog ? { focus() { focused = sel; } } : $(sel, root));
const chosen = sandbox.chooseReset();
assert.equal(focused, "[data-ask=keep]", "keeping the files is not the reset's default");
assert.match(dialog.innerHTML, /a fresh one takes its place/, "the reset does not say the person keeps a computer");
dialog.close("keep");
assert.equal(await chosen, "keep");
console.log("one computer ok");
