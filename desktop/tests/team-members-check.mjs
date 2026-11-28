// Adding and removing a workspace's members from inside it (the owner,
// 2026-11-12: "team has no way to add or delete members from a team
// workspace"). The people page was reachable only from a small link at the
// foot of the rail, below every channel and conversation; the right panel's
// Members showed initials and nothing to do. Now the way there sits under
// Activity at the top of the rail and on the panel's Members, saying what a
// Team Admin does there; a plain member sees who is in and no controls;
// removing asks first; and an invite is sent once however often the page
// redraws.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
let confirm = true;
const home = {
  id: "w", name: "Alpha", role: "owner", me: "dana", me_name: "Dana", agent: { id: "agent:me", name: "My Magnus", mention: "Magnus" }, computer: null,
  members: [{ id: "dana", name: "Dana", role: "owner", online: true }, { id: "sam", name: "Sam", role: "admin", online: true }, { id: "leo", name: "Leo", role: "member", online: false }],
  channels: [{ id: "c1", name: "general", project: null, mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: null }],
  projects: [],
};
const sandbox = {
  UI: {}, route: () => {}, esc, enc: encodeURIComponent, $$: () => [], paint: () => true, when: () => "", firstLine: s => String(s || ""),
  $: (sel, root) => root?.q?.(sel) ?? null,
  icon: k => `<svg data-icon="${k}"></svg>`, toast: () => {}, ask: async () => confirm, kb: n => `${n} B`,
  Magnus: { kind: "web" }, Compose: { markdown: s => esc(s), maskedText: s => esc(s) },
  api: async (m, p, b) => {
    calls.push([m, p, b]);
    if (m === "GET" && p.endsWith("/home")) return home;
    if (m === "GET" && p.endsWith("/people")) return [{ id: "kim", name: "Kim" }];
    return { emailed: true };
  },
  localStorage: { getItem: () => null, setItem: () => {} },
  document: { addEventListener: () => {}, hidden: false, hasFocus: () => false, body: { append() {} } },
  location: { hash: "#/team/w/people", origin: "https://server", pathname: "/" }, setTimeout: () => 0, clearTimeout: () => {}, console,
};
sandbox.globalThis = sandbox;
const files = ["server.js", "teams.js", "team-chat.js", "coedit.js", "team-panel.js", "team-command.js", "team-people.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.Magnus = Object.assign(globalThis.Magnus || {}, { api: (m, p, b) => api(m, p, b) });\nglobalThis.__x = { Team };`, sandbox);
const { Team } = sandbox.__x;
Team.home = home; Team.ws = "w"; Team.homes = { w: home };

const owner = home, admin = { ...home, role: "admin", me: "sam" }, member = { ...home, role: "member", me: "leo" }, guest = { ...home, role: "guest", me: "leo" };

// The shared conversation header includes a direct Members chip opening
// shared:settings in the unified Canvas, and legacy #/team/:ws/people resolves there.
for (const h of [owner, admin, member]) {
  const tools = sandbox.channelTools(h, h.channels[0], null);
  assert.match(tools, /data-open="shared:settings"[^]*>3<\/span>/, `header Members chip missing count for ${h.role}`);
}
assert.equal(sandbox.sharedDeepLinkPanel({ kind: "people" }), "shared:settings");

// The right panel's Members: names, not just initials, and any collaborator
// gets the link to add or invite peers; guests see who is in.
const panel = { project: null, channels: [], files: [], tasks: [], work: [], members: [{ id: "dana", name: "Dana" }, { id: "leo", name: "Leo" }], knowledge: 0, progress: null, computer: null };
Team.home = owner;
const card = sandbox.membersCardHtml(panel, "w").split("data-members-card")[1].split("</section>")[0];
assert.match(card, /href="#\/team\/w\/people" data-members-manage>Add or invite</, "the owner's panel offers no way to add or invite peers");
assert.match(card, /title="Leo"[^]*class="sr">Leo</, "the panel's members are initials with no names");
Team.home = member;
assert.match(sandbox.membersCardHtml(panel, "w"), /data-members-manage>Add or invite</, "a peer collaborator cannot add or invite teammates");
Team.home = guest;
assert.match(sandbox.membersCardHtml(panel, "w"), /data-members-manage>See all</, "a guest cannot see who is in");
assert.doesNotMatch(sandbox.membersCardHtml(panel, "w"), /Add or invite/, "a guest is offered to add people");
Team.home = owner;

// Pure peer Project Settings: any collaborator (creator or peer) gets ONE
// unified Name or email input, 1-click suggestion chips, Copy invite link, and
// a merged roster with zero role dropdowns, zero Shared Apps, and zero Team Agent row.
const caps = [{ id: "leo", name: "Leo", apps: [{ name: "linear", title: "Linear" }] }];
for (const h of [owner, admin, member]) {
  const page = sandbox.teamPeopleHtml(h, [{ id: "kim", name: "Kim" }], caps);
  assert.match(page, /data-people-input/, `a ${h.role} is missing the unified Name or email input`);
  assert.match(page, /data-quick-add="kim"[^>]*>\+ Kim/, `a ${h.role} is missing 1-click server candidate chips`);
  assert.match(page, /data-invite-link/, `a ${h.role} cannot copy an invite link`);
  assert.match(page, /Linear/, `merged peer roster missing advertised personal apps for ${h.role}`);
  assert.doesNotMatch(page, /data-role|data-add-role|data-team-apps|team agent, in every channel/, `retired admin/shared-agent UI leaked for ${h.role}`);
  assert.ok(page.indexOf("data-people-add") < page.indexOf("data-peer-roster"), "adding people is below the roster");
}
assert.match(sandbox.teamPeopleHtml(owner, [], caps).split('data-member="leo"')[1].split("data-peer-roster")[0], /data-remove/, "creator cannot remove a member");
assert.doesNotMatch(sandbox.teamPeopleHtml(member, [], caps), /data-remove|data-delete-workspace/, "a non-creator peer is offered Remove or Delete project");
assert.match(sandbox.teamPeopleHtml(member, [], caps), /data-leave/, "a peer cannot leave from the Danger Zone");

// Guests see the peer roster only with no add/invite/delete controls.
const guestPage = sandbox.teamPeopleHtml(guest, [], caps);
for (const id of ["dana", "sam", "leo"]) assert.match(guestPage, new RegExp(`data-member="${id}"`), "a guest does not see who is in");
assert.doesNotMatch(guestPage, /data-people-input|data-quick-add|data-invite-link|data-remove|data-delete-workspace/, "a guest is offered to change who is in");

// Wiring: 1-click chip add, email invite, remove confirmation, single-fire listeners across redraws.
const listeners = [];
const field = { value: "ann@example.com", focus() {} };
const result = { textContent: "" };
const center = {
  dataset: {},
  addEventListener: (type, fn) => listeners.push([type, fn]),
  q: sel => (sel === "[data-people-input]" ? field : sel === "[data-invite-result]" ? result : null),
};
await sandbox.teamPeople(center, {}, "w");
const fire = async (type, ev) => { for (const [t, fn] of listeners) if (t === type) await fn(ev); };

// Remove cancelled vs confirmed.
const removeEv = { target: { closest: s => (s === "[data-remove]" ? {} : s === "[data-member]" ? { dataset: { member: "leo", name: "Leo" } } : null) } };
confirm = false; calls.length = 0;
await fire("click", removeEv);
assert.deepEqual(calls.filter(c => c[0] === "DELETE"), [], "a member was removed though the confirmation was cancelled");
confirm = true;
await fire("click", removeEv);
assert.deepEqual(calls.filter(c => c[0] === "DELETE"), [["DELETE", "/v1/workspaces/w/members/leo", undefined]], "removing a member did not reach the server");
assert.equal(listeners.filter(([t]) => t === "submit").length, 1, "repeated redraws bound multiple submit listeners");

// 1-click chip immediately adds candidate as member.
calls.length = 0;
await fire("click", { target: { closest: s => (s === "[data-quick-add]" ? { dataset: { quickAdd: "kim", quickName: "Kim" } } : null) } });
assert.equal(JSON.stringify(calls.filter(c => c[0] === "POST")), JSON.stringify([["POST", "/v1/workspaces/w/members", { person: "kim", role: "member" }]]), "1-click chip did not add candidate");

// Unified input submits email once and reloads roster.
calls.length = 0; field.value = "ann@example.com";
const form = { q: sel => (sel === "[data-people-input]" ? field : null) };
await fire("submit", { target: { closest: s => (s === "[data-people-invite]" ? form : null) }, preventDefault() {} });
assert.equal(JSON.stringify(calls.filter(c => c[0] === "POST")), JSON.stringify([["POST", "/v1/workspaces/w/invites", { email: "ann@example.com" }]]), "an invite was not sent exactly once");
assert.match(result.textContent, /Invitation emailed to ann@example\.com/);
// The outcome is drawn with the panel, so the redraw after an invite (and
// every refresh) keeps it instead of wiping the link to send.
assert.match(sandbox.teamPeopleHtml({ ...owner, id: "w" }, [], caps), /data-invite-result[^>]*>Invitation emailed to ann@example\.com/, "a redraw wiped the invite's outcome");

// Switching workspaces rebinds context without adding duplicate listeners.
sandbox.wirePeopleSettings(center, "v", { ...owner, id: "v" }, [], async () => {});
assert.equal(listeners.filter(([t]) => t === "submit").length, 1, "switching workspaces added a second invite listener");
calls.length = 0; field.value = "bo@example.com";
await fire("submit", { target: { closest: s => (s === "[data-people-invite]" ? form : null) }, preventDefault() {} });
assert.equal(JSON.stringify(calls.filter(c => c[0] === "POST").map(c => c[1])), JSON.stringify(["/v1/workspaces/v/invites"]), "after a switch, an invite went to the workspace shown before");

console.log("team members check passed");
