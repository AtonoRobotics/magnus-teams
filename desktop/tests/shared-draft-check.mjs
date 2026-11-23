// A shared project's conversation keeps its unsent message as the personal
// chat does (#544): in memory for the session, nothing on the device,
// forgotten at sign-out, and never shown to another person who signs in on
// the same computer. An earlier version kept it in localStorage
// (`team-draft:`), unkeyed by person and kept through sign-out; what it left
// on a device goes too.
//
// The shared conversation is opened as the app opens it (TeamChat.mount),
// with the real chat view and composer, on a fake page, and typed into.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import vm from "node:vm";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const read = f => readFileSync(join(ui, f), "utf8");
const page = [...read("index.html").matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
const wanted = ["chat-view.js", "chat-composer.js", "teams.js", "team-chat.js", "team-conversation.js"];
const scripts = page.filter(f => wanted.includes(f)).map(read);
// The draft store, from whichever script the page declares it in.
const DRAFTS = /^const MessageDrafts = \(\(\) => \{[\s\S]*?^\}\)\(\);$/m;
if (!scripts.some(s => DRAFTS.test(s))) {
  const home = page.find(f => DRAFTS.test(read(f)));
  assert.ok(home, "no script the page loads declares MessageDrafts");
  scripts.unshift(read(home).match(DRAFTS)[0]);
}

/** localStorage as a device keeps it: through sign-out and restarts. */
function deviceStorage() {
  const m = new Map();
  return {
    m,
    getItem: k => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: k => m.delete(k),
    key: i => [...m.keys()][i] ?? null, get length() { return m.size; },
  };
}

/** A page element that holds what is set on it; each selector finds one child. */
function element() {
  const kids = new Map();
  return {
    value: "", style: {}, dataset: {}, innerHTML: "", isConnected: true, selectionStart: 0, selectionEnd: 0, scrollHeight: 0,
    classList: { toggle() {}, add() {}, remove() {}, contains: () => false },
    setAttribute() {}, removeAttribute() {}, setSelectionRange() {}, focus() {}, addEventListener() {}, dispatchEvent() {},
    matches: () => false, closest: () => null, querySelectorAll: () => [],
    querySelector(sel) { if (!kids.has(sel)) kids.set(sel, element()); return kids.get(sel); },
  };
}

const home = {
  id: "w", name: "Alpha", role: "member", me: "alice", agent: { id: "agent", name: "Magnus" },
  members: [{ id: "alice", name: "Alice" }, { id: "bob", name: "Bob" }],
  channels: [{ id: "c1", name: "launch", project: null, mode: "mentions", unread: 0, direct: null }], projects: [],
};

/** The app as it starts on this computer: its scripts, over the device's storage. */
function start(localStorage) {
  const left = [];
  const sandbox = {
    localStorage, crypto: { randomUUID }, console, setTimeout, clearTimeout, innerHeight: 900, window: {}, UI: {}, App: {},
    location: { hash: "#/team/w/c/c1" }, document: { addEventListener() {} }, route: () => {},
    $: (sel, root) => root?.querySelector(sel) ?? null, $$: () => [], paint: (el, html) => { if (el) el.innerHTML = html; return true; },
    esc: s => String(s ?? ""), enc: encodeURIComponent, icon: () => "", attachButton: () => "", toast() {}, when: () => "", firstLine: s => String(s || "").split("\n")[0],
    modelPicker: async () => `<label class="model"><select data-model data-default=""></select></label>`, chosenModel: () => undefined,
    onLeave: f => left.push(f), ReadingAnchors: { watch: () => () => {} },
    Server: { channels: { typing: async () => ({}) }, tasks: { cancel: async () => ({}) } }, Magnus: {},
  };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(`${scripts.join("\n")}\nglobalThis.__x = { MessageDrafts, Team, TeamChat };`, sandbox, { filename: "shared-draft" });
  const { Team, TeamChat } = sandbox.__x;
  Team.home = home; Team.ws = "w";
  return {
    drafts: sandbox.__x.MessageDrafts,
    /** Open the shared conversation; the text box as it shows. */
    async open() {
      TeamChat.s = { ws: "w", channel: "c1", thread: null, root: element(), view: { messages: [], typing: [], names: {} }, files: {}, whispers: [], hidden: new Set(), typedAt: 0, views: [] };
      const view = await TeamChat.mount(TeamChat.s.root, null);
      return sandbox.$("textarea", sandbox.$("form.say", sandbox.$(".composer-slot", view.section)));
    },
    /** Move away: the composer lets go of the box. */
    leave() { for (const f of left.splice(0)) f(); },
  };
}

const device = deviceStorage();
const secret = "Draft offer: 12% discount for Acme";

// Alice types in the shared conversation and moves around the app: the
// message stays with the conversation, in memory.
let app = start(device);
app.drafts.open("server-1", "alice");
let box = await app.open();
box.value = secret;
box.oninput();
app.leave();
box = await app.open();
assert.equal(box.value, secret, "a shared conversation loses what was typed when the person moves away and back");
assert.deepEqual([...device.m].filter(([, v]) => v.includes("12% discount")), [], "a shared conversation's unsent message is written to the device");

// Alice signs out: the unsent message is gone, for her too.
app.leave();
app.drafts.clear();
box = await app.open();
assert.equal(box.value, "", "a shared conversation's unsent message survives sign-out");

// Alice types again, then the app restarts and Bob signs in on the same computer.
box.value = secret;
box.oninput();
app.leave();
app = start(device);
app.drafts.open("server-1", "bob");
box = await app.open();
assert.equal(box.value, "", "another person on the same computer reads a shared conversation's unsent message");

// What an earlier version left on the device is not shown, and goes.
device.setItem("team-draft:w:c1:", secret);
app.leave();
app = start(device);
app.drafts.open("server-1", "bob");
box = await app.open();
assert.equal(box.value, "", "an unsent message an earlier version left on the device is shown");
assert.equal(device.getItem("team-draft:w:c1:"), null, "an unsent message an earlier version left on the device stays there");
app.leave();

console.log("shared-project drafts: in memory, nothing on the device, gone at sign-out, not shown to another person passed");
