// Admin › People (spec rev 141): the four roles, invitations by email or
// link, turning off, signing out everywhere, removal that says what becomes
// of their projects and agents, the last owner's refusal shown, and no way
// to read a person's data (the owner, 2026-11-11).
import assert from "node:assert/strict";
import vm from "node:vm";
import { wired, load, draw, node, refused, same } from "./admin-harness.mjs";

wired("admin-people.js", "admin-people-check.mjs");

const people = [
  { id: "sam", name: "Sam", email: null, role: "owner", status: "active", last_sign_in: null, sessions: 2 },
  { id: "mo", name: "Mo", email: "mo@example.com", role: "member", status: "active", last_sign_in: null, sessions: 1 },
  { id: "gus", name: "Gus", email: "gus@example.com", role: "guest", status: "invited", last_sign_in: null, sessions: 0 },
];
const answers = {
  "GET /v1/admin/people": { people },
  "POST /v1/admin/people": b => ({ id: "new", email: b.email, invite_token: "t", link: "https://magnus.example/invite?token=t", emailed: b.send }),
  "POST /v1/admin/people/sam/disable": refused(409, { error: "last_owner", message: "This is the last owner: make someone else an owner first." }),
  "POST /v1/admin/people/mo/sign-out": { ended: 1 },
};

const ctx = load("admin-people.js", answers);
const el = await draw(ctx, "people");
const row = id => el.html.split(`data-person="${id}"`)[1].split("</tr>")[0];
for (const [r, label] of [["owner", "Owner"], ["admin", "Admin"], ["member", "Member"], ["guest", "Guest: reads and chats only"]]) {
  assert.ok(row("sam").includes(`value="${r}"`), `the owner's row offers ${r}`);
  assert.ok(el.html.includes(label), `the page names the role ${label}`);
}
assert.ok(!row("mo").includes('value="owner"'), "an admin who is not an owner is offered the owner role");
assert.match(row("sam"), /<select data-people-role[^>]* disabled>/, "an admin could change an owner's role");
assert.match(row("gus"), /Withdraw/, "an invitation can be withdrawn");
assert.match(row("mo"), /Sign out everywhere/);

// Role.
const moRow = node({ "data-person": "mo" });
await el.onchange({ target: node({ "data-people-role": "", value: "guest" }, moRow) });
same(ctx.calls.find(c => c[0] === "PATCH"), ["PATCH", "/v1/admin/people/mo", { role: "guest" }]);

// Sign out everywhere.
await el.onclick({ target: node({ "data-people-act": "sign-out" }, moRow) });
assert.ok(ctx.calls.some(c => c[0] === "POST" && c[1] === "/v1/admin/people/mo/sign-out"));
assert.ok(ctx.toasts.includes("Signed out of 1 device."), ctx.toasts.join(" | "));

// The last owner's refusal is said, not swallowed.
await el.onclick({ target: node({ "data-people-act": "disable" }, node({ "data-person": "sam" })) });
assert.ok(ctx.toasts.some(t => t.includes("last owner")), "the last owner's refusal was not shown");

// Removal asks what becomes of their projects and agents.
vm.runInContext("peopleRemoveSheet = async (p) => ({ projects: 'transfer', to: 'sam' });", ctx);
await el.onclick({ target: node({ "data-people-act": "remove" }, moRow) });
same(ctx.calls.find(c => c[0] === "DELETE"), ["DELETE", "/v1/admin/people/mo", { projects: "transfer", to: "sam" }]);

// Invite: the link to pass on, and whether it was emailed.
const form = ctx.$("[data-people-invite]");
form.fields = { name: "Ann", email: "ann@example.com", role: "guest", send: "on" };
await form.onsubmit({ preventDefault() {}, target: form });
same(ctx.calls.find(c => c[0] === "POST" && c[1] === "/v1/admin/people")[2], { name: "Ann", email: "ann@example.com", role: "guest", send: true });
assert.match(ctx.$("[data-people-note]").html, /We emailed them the link\./);
assert.match(ctx.$("[data-people-note]").html, /https:\/\/magnus\.example\/invite\?token=t/);

// Invite by link, with no email: the link and who it makes; with no public
// address, said plainly, never a raw code.
const invited = r => vm.runInContext(`peopleInvited(${JSON.stringify(r)})`, ctx);
const byLink = invited({ by_link: true, role: "member", link: "https://magnus.example/invite?link=c", emailed: false });
assert.match(byLink, /makes their own account as member/);
assert.match(byLink, /invite\?link=c/);
const noBase = invited({ by_link: true, role: "member", link: null, emailed: false });
assert.match(noBase, /no public address/, "a link-less server does not say why there is no link");
assert.doesNotMatch(invited({ id: "x", invite_token: "t", link: null, emailed: false }), /Invitation code|>t</, "a raw invitation code is shown");
assert.doesNotMatch(vm.runInContext("peopleHtml([], true)", ctx), /name="email"[^>]*required/, "an email address is still required, so there is no link-only invite");

// A Magnus admin supports accounts and reads no one's data.
assert.doesNotMatch(el.html, /data-people-act="data"|Their data|data-people-read/, "the admin is offered a person's data");

// The removal sheet itself says their own data is deleted and offers both.
const src = (await import("node:fs")).readFileSync(new URL("../ui/admin-people.js", import.meta.url), "utf8");
assert.match(src, /Their own chats, tasks, files and memory are deleted/);
assert.match(src, /value="transfer" checked> Give them to/);
assert.match(src, /value="delete"> Delete them/);
console.log("admin people check passed");
