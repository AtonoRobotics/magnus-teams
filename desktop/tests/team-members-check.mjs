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
  id: "w", name: "Alpha", role: "owner", me: "dana", me_name: "Dana", agent: { id: "agent", name: "Magnus" }, computer: null,
  members: [{ id: "dana", name: "Dana", role: "owner", online: true }, { id: "sam", name: "Sam", role: "admin", online: true }, { id: "leo", name: "Leo", role: "member", online: false }],
  channels: [{ id: "c1", name: "general", project: null, mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: null }],
  projects: [],
};
const sandbox = {
  teamApps: async () => {}, UI: {}, route: () => {}, esc, enc: encodeURIComponent, $$: () => [], paint: () => true, when: () => "", firstLine: s => String(s || ""),
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
const files = ["teams.js", "team-chat.js", "team-panel.js", "team-command.js", "team-people.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { Team };`, sandbox);
const { Team } = sandbox.__x;
Team.home = home; Team.ws = "w"; Team.homes = { w: home };

const owner = home, admin = { ...home, role: "admin", me: "sam" }, member = { ...home, role: "member", me: "leo" }, guest = { ...home, role: "guest", me: "leo" };

// The rail: the people page is the second thing on it, above the projects,
// channels and conversations that pushed it out of sight; it says what a
// Team Admin does there.
for (const h of [owner, admin, member]) {
  const rail = sandbox.railHtml(h, { kind: "channel", id: "c1" });
  const link = rail.match(/<a href="#\/team\/w\/people"[^>]*data-rail-people>([^]*?)<\/a>/);
  assert.ok(link, `the rail has no way to the people page for a workspace ${h.role}`);
  assert.ok(rail.indexOf("data-rail-people") < rail.indexOf("<h3>Projects</h3>"), "the way to the people page is below the projects and channels");
  assert.match(link[1], /3<\/span>$/, "the rail does not say how many people are in");
  if (h.role === "member") assert.doesNotMatch(link[1], /add or remove/, "a member is told they add or remove people");
  else assert.match(link[1], /add or remove/, `a ${h.role} is not told where to add or remove people`);
}
assert.equal(sandbox.railHtml(owner, { kind: "channel", id: "c1" }).match(/#\/team\/w\/people/g).length, 1, "the rail links the people page twice");
assert.match(sandbox.railHtml(owner, { kind: "people" }), /aria-current="page" data-rail-people/, "the rail does not mark the people page as open");

// The right panel's Members: names, not just initials, and the way to add
// or remove for a Team Admin; for anyone else, to see who is in.
const panel = { project: null, channels: [], files: [], tasks: [], work: [], members: [{ id: "dana", name: "Dana" }, { id: "leo", name: "Leo" }], knowledge: 0, progress: null, computer: null };
Team.home = owner;
const card = sandbox.overviewHtml(panel, "w").split("data-members-card")[1].split("</section>")[0];
assert.match(card, /href="#\/team\/w\/people" data-members-manage>Add or remove</, "the owner's panel offers no way to add or remove members");
assert.match(card, /title="Leo"[^]*class="sr">Leo</, "the panel's members are initials with no names");
Team.home = admin;
assert.match(sandbox.overviewHtml(panel, "w"), /data-members-manage>Add or remove</, "a workspace admin's panel offers no way to add or remove members");
Team.home = member;
assert.match(sandbox.overviewHtml(panel, "w"), /data-members-manage>See all</, "a member cannot see who is in");
assert.doesNotMatch(sandbox.overviewHtml(panel, "w"), /Add or remove/, "a member is offered to add or remove people");
Team.home = owner;

// The people page: a Team Admin adds a server person, invites by email or
// link, changes roles and removes; a member or guest sees the list only.
for (const h of [owner, admin]) {
  const page = sandbox.teamPeopleHtml(h, [{ id: "kim", name: "Kim" }]);
  assert.match(page, /data-add-people/, `a ${h.role} cannot add someone on the server`);
  assert.match(page, /data-invite-email/, `a ${h.role} cannot invite by email`);
  assert.match(page, /data-invite-link/, `a ${h.role} cannot copy an invite link`);
  assert.match(page.split('data-member="leo"')[1].split("</li>")[0], /data-remove/, `a ${h.role} cannot remove a member`);
  assert.ok(page.indexOf("data-people-add") < page.indexOf('class="team-people"'), "adding people is below the whole list");
}
for (const h of [member, guest]) {
  const page = sandbox.teamPeopleHtml(h, []);
  for (const id of ["dana", "sam", "leo"]) assert.match(page, new RegExp(`data-member="${id}"`), `a ${h.role} does not see who is in`);
  assert.doesNotMatch(page, /data-add-people|data-invite|data-remove|data-role|data-delete-workspace/, `a ${h.role} is offered to change who is in`);
  assert.match(page, /A Team Admin adds and removes people here/, `a ${h.role} is not told who changes who is in`);
}

// The page at work: removing asks first and does nothing when cancelled;
// an invite goes once, however often the page redraws.
const listeners = [];
const center = { dataset: {}, addEventListener: (type, fn) => listeners.push([type, fn]) };
await sandbox.teamPeople(center, {}, "w");
const target = sel => ({ closest: s => (s === sel ? { dataset: { member: "leo", name: "Leo" } } : s === "[data-member]" ? { dataset: { member: "leo", name: "Leo" } } : null) });
confirm = false; calls.length = 0;
await center.onclick({ target: target("[data-remove]") });
assert.deepEqual(calls.filter(c => c[0] === "DELETE"), [], "a member was removed though the confirmation was cancelled");
confirm = true;
await center.onclick({ target: target("[data-remove]") });
assert.deepEqual(calls.filter(c => c[0] === "DELETE"), [["DELETE", "/v1/workspaces/w/members/leo", undefined]], "removing a member did not reach the server");
assert.equal(listeners.filter(([t]) => t === "submit").length, 1, "the page listens for invites more than once after a redraw, so each invite goes twice");
const email = { value: "ann@example.com" }, result = { textContent: "" };
const form = { q: sel => (sel === "[data-invite-email]" ? email : sel === "[data-invite-result]" ? result : null) };
calls.length = 0;
for (const [t, fn] of listeners) if (t === "submit") await fn({ target: { closest: s => (s === "[data-invite]" ? form : null) }, preventDefault() {} });
assert.equal(JSON.stringify(calls.filter(c => c[0] === "POST")), JSON.stringify([["POST", "/v1/workspaces/w/invites", { email: "ann@example.com" }]]), "an invite was not sent exactly once");
assert.match(result.textContent, /Invitation emailed to ann@example\.com/);

// The same element shows another workspace's page after a switch: its
// invites go to that workspace alone, never to the one shown before.
sandbox.bindInvite(center, "v");
assert.equal(listeners.filter(([t]) => t === "submit").length, 1, "switching workspaces added a second invite listener");
calls.length = 0; email.value = "bo@example.com";
for (const [t, fn] of listeners) if (t === "submit") await fn({ target: { closest: s => (s === "[data-invite]" ? form : null) }, preventDefault() {} });
assert.equal(JSON.stringify(calls.filter(c => c[0] === "POST").map(c => c[1])), JSON.stringify(["/v1/workspaces/v/invites"]), "after a switch, an invite went to the workspace shown before");

console.log("team members check passed");
