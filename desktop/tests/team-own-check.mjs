// What each person keeps of a workspace, as the UI draws it: archived and
// deleted conversations off the rail, a deleted message's place, deleting
// one's own message, the people page's rules (add, remove, leave, delete),
// the Admin › People › Workspaces list, and the pop-ups with what they say.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [], told = [], store = {};
const body = { children: [], append(el) { this.children.push(el); } };
const sandbox = {
  UI: {}, route: () => {}, esc, enc: encodeURIComponent, $: () => null, $$: () => [], paint: () => true, when: t => (t ? "now" : ""), firstLine: s => String(s || "").split("\n")[0],
  icon: k => `<svg data-icon="${k}"></svg>`, toast: () => {}, ask: async () => true, kb: n => `${n} B`,
  Magnus: { kind: "web", notifyPermission: () => "granted", notify: (t, b) => told.push([t, b]) },
  Compose: { markdown: s => `<p>${esc(s)}</p>`, maskedText: s => esc(s) },
  api: async (m, p, b) => { calls.push([m, p, b]); return {}; },
  localStorage: { getItem: k => store[k] ?? null, setItem: (k, v) => { store[k] = String(v); } },
  document: { addEventListener: () => {}, hidden: false, hasFocus: () => false, body },
  location: { hash: "#/inbox", origin: "https://server", pathname: "/" }, setTimeout: () => 0, clearTimeout: () => {}, console,
};
sandbox.globalThis = sandbox;
const files = ["teams.js", "team-chat.js", "team-panel.js", "team-command.js", "team-people.js", "notices.js", "admin-workspaces.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { Team, Notices };`, sandbox);
const { Team, Notices } = sandbox.__x;

const home = {
  id: "w", name: "Alpha", role: "admin", me: "sam", me_name: "Sam", agent: { id: "agent", name: "Magnus" }, computer: null,
  members: [{ id: "sam", name: "Sam", role: "admin", online: true }, { id: "dana", name: "Dana", role: "owner", online: true }, { id: "leo", name: "Leo", role: "member", online: false }],
  channels: [
    { id: "c1", name: "general", project: null, mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: null },
    { id: "c2", name: "random", project: null, mode: "mentions", notify: "all", unread: 2, mentions: 0, direct: null, archived: true },
    { id: "d1", name: "", project: null, mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: { people: ["dana", "sam"], agent: false }, deleted: true },
  ],
  projects: [],
};
Team.home = home; Team.ws = "w";

// The rail: an archived channel is folded away, a deleted conversation gone.
const rail = sandbox.railHtml(home, { kind: "channel", id: "c1" });
const main = rail.split("team-archived")[0];
assert.doesNotMatch(main, /#random/, "an archived channel stays on the list");
assert.match(rail, /<details class="team-archived"><summary class="small">Archived \(1\)<\/summary>[^]*#random/, "an archived channel cannot be found again");
assert.doesNotMatch(rail, /data-channel="d1"/, "a deleted conversation stays on the list");
assert.match(rail, /data-dm="dana"/, "with the conversation deleted, Dana cannot be written to again");
assert.match(sandbox.railHtml(home, { kind: "channel", id: "c2" }).split("team-archived")[0], /#random/, "the archived channel open now is not on the list");
assert.equal(sandbox.firstChannel({ ...home, channels: [home.channels[1], home.channels[0]] }), "c1", "a workspace opened on an archived channel");

// A deleted message keeps its place only where it has replies; one's own
// message can be deleted, someone else's cannot.
const ctx = { me: "sam", ws: "w", channel: "c1", name: id => id, guest: false };
const gone = sandbox.messageHtml({ id: "m1", sender: "dana", text: "", thread: null, at: "", deleted: true, replies: 2 }, ctx);
assert.match(gone, /This message was deleted\.[^]*2 replies/);
assert.doesNotMatch(gone, /data-react|data-edit/, "a deleted message offers actions");
assert.match(sandbox.messageHtml({ id: "m2", sender: "sam", text: "mine", thread: null, at: "" }, ctx), /data-delete-msg/, "one's own message cannot be deleted");
assert.doesNotMatch(sandbox.messageHtml({ id: "m3", sender: "dana", text: "hers", thread: null, at: "" }, ctx), /data-delete-msg/, "someone else's message can be deleted");
const view = { messages: [{ id: "a", text: "x" }, { id: "b", text: "y", replies: 1 }] };
sandbox.applyLocal(view, { op: "delete", id: "a" });
sandbox.applyLocal(view, { op: "delete", id: "b" });
assert.deepEqual(view.messages.map(m => [m.id, !!m.deleted]), [["b", true]], "a deleted message did not leave at once");

// The people page: an admin manages members and admins, not the owner; the
// last owner cannot leave; people on the server are offered by name.
const page = sandbox.teamPeopleHtml(home, [{ id: "kim", name: "Kim" }], false);
const of = id => page.split(`data-member="${id}"`)[1].split("</li>")[0];
assert.doesNotMatch(of("dana"), /data-remove|data-role/, "an admin is offered to change the owner");
assert.match(of("leo"), /data-remove/);
assert.doesNotMatch(of("leo"), /<option >owner<\/option>/, "an admin is offered a role above their own");
assert.match(of("sam"), /data-leave/, "a member cannot leave");
assert.match(page, /data-add-person[^]*value="kim">Kim/, "people on the server are not offered");
assert.doesNotMatch(page, /data-delete-workspace/, "a workspace admin is offered to delete it");
const owners = sandbox.teamPeopleHtml({ ...home, role: "owner", me: "dana" }, [], false);
assert.match(owners, /data-delete-workspace/, "the owner cannot delete the workspace");
assert.doesNotMatch(owners.split('data-member="dana"')[1].split("</li>")[0], /data-leave/, "the only owner is offered to leave");
assert.match(sandbox.teamPeopleHtml(home, [], true), /data-delete-workspace/, "an admin of the server cannot delete it");

// Admin › People › Workspaces: every workspace, its only owner kept.
const ws = sandbox.adminWorkspacesHtml([{ id: "w", name: "Alpha", channels: 3, members: [{ id: "dana", name: "Dana", role: "owner" }, { id: "leo", name: "Leo", role: "member" }] }], [{ id: "kim", name: "Kim" }, { id: "leo", name: "Leo" }]);
assert.match(ws.split('data-ws-member="dana"')[1].split("</tr>")[0], /disabled[^]*Its only owner/, "the only owner can be removed");
assert.match(ws, /data-ws-add[^]*value="kim"/);
assert.doesNotMatch(ws.split("data-ws-add")[1], /value="leo"/, "a member is offered to be added again");

// Pop-ups: who and where; the system's notification never says what a
// message says; nothing twice; and not what the person is looking at.
const n = { id: "m9", kind: "direct", workspace: "w", workspace_name: "Alpha", channel: "d1", channel_name: "", direct: ["dana"], thread: null, sender: "dana", at: "", preview: "the code is 4471" };
const shown = sandbox.noticeWords(n, { dana: "Dana" });
assert.deepEqual([shown.title, shown.body, shown.system, shown.href], ["Dana", "the code is 4471", "sent you a message", "#/team/w/c/d1"]);
const mention = sandbox.noticeWords({ ...n, kind: "mention", direct: null, channel: "c1", channel_name: "general", thread: "m1" }, { dana: "Dana" });
assert.deepEqual([mention.title, mention.system, mention.href], ["Dana mentioned you", "in #general, Alpha", "#/team/w/c/c1/t/m1"]);
for (const kind of ["direct", "mention", "message", "answer"]) assert.doesNotMatch(sandbox.noticeWords({ ...n, kind }, {}).system, /4471/, `a ${kind} told the system what it says`);
// A stand-in for the DOM's elements, enough for popup(): each knows its parent.
let box = null;
sandbox.document.createElement = tag => {
  const el = { tag, className: "", dataset: {}, attrs: {}, children: [], parent: null, setAttribute(k, v) { this.attrs[k] = v; }, addEventListener() {},
    append(c) { c.parent = this; this.children.push(c); }, get firstElementChild() { return this.children[0]; },
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this); this.parent = null; } };
  return el;
};
body.append = el => { box = el; };
sandbox.$ = sel => sel === "#notices" ? box : null;
const popups = () => (box?.children || []).filter(c => c.className === "notice");
for (let i = 0; i < 5; i++) Notices.show(sandbox.noticeWords({ ...n, id: `m${i}` }, { dana: "Dana" }));
Notices.show(sandbox.noticeWords({ ...n, id: "m4" }, { dana: "Dana" }));
assert.ok(popups().length <= 3, `pop-ups piled up: ${popups().length}`);
assert.equal(told.length, 5, "a notice went to the system twice, or not while the app was in the background");
assert.ok(told.every(([, b]) => b === "sent you a message"));
told.length = 0;
sandbox.document.hasFocus = () => true;
sandbox.location.hash = "#/team/w/c/d1";
Notices.show(sandbox.noticeWords({ ...n, id: "m7" }, { dana: "Dana" }));
assert.equal(told.length, 0, "the system was told of a message the person is looking at");
sandbox.location.hash = "#/inbox";
Notices.show(sandbox.noticeWords({ ...n, id: "m8" }, { dana: "Dana" }));
assert.equal(told.length, 0, "the system was told while the app is in front");
// Turned off on this device, the system is not told; a browser not yet
// asked is offered the choice from a pop-up, never asked by itself.
sandbox.document.hasFocus = () => false;
Notices.setSystem(false);
Notices.show(sandbox.noticeWords({ ...n, id: "m10" }, { dana: "Dana" }));
assert.equal(told.length, 0, "the system was told with its notifications turned off here");
sandbox.Magnus.notifyPermission = () => "default";
assert.equal(Notices.offer(), true, "a browser not yet asked is not offered system notifications");
assert.equal(Notices.systemOn(), false);
console.log("team own ok");
