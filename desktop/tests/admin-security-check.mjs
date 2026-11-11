// Admin › Security (spec rev 141): the sign-in policy, the audit log with
// filter and export, how long things are kept, and developer-access sessions.
import assert from "node:assert/strict";
import vm from "node:vm";
import { wired, load, draw, same } from "./admin-harness.mjs";

wired("admin-security.js", "admin-security-check.mjs");

const policy = { code_required: true, idle_minutes: 30, max_hours: 12, lockout_enabled: true, lockout_failures: 5, lockout_wait_minutes: 1, lockout_max_wait_minutes: 60, lockout_permanent: true, identity_provider: "Keycloak" };
const later = new Date(Date.now() + 3600e3).toISOString();
const ctx = load("admin-security.js", {
  "GET /v1/admin/sign-in-policy": policy,
  "GET /v1/admin/audit": { entries: [{ seq: 2, at: null, admin: "admin:sam", name: "Sam", action: "person_data_read", target: "mo", source: null }] },
  "GET /v1/admin/audit?actor=sam": { entries: [] },
  "GET /v1/admin/audit/export?actor=sam": { filename: "audit.csv", csv: "seq,at\n" },
  "GET /v1/admin/retention": { kept: [{ what: "chats", how: "Until the person deletes them." }] },
  "GET /v1/admin/developer-access": { sessions: [
    { computer: "c1", owner: { person: "mo" }, operator: "dev@magnus", until: later, ended: false },
    { computer: "c2", owner: { workspace: "Shop" }, operator: "dev2@magnus", until: later, ended: true },
  ] },
});
const el = await draw(ctx, "security");
assert.match(el.html, /People sign in through Keycloak\./);
assert.match(el.html, /name="code_required" checked/);
assert.match(el.html, /name="idle_minutes" min="5" max="1440" value="30"/);
assert.match(el.html, /<td class="small">Sam<\/td><td class="small mono">person_data_read<\/td><td class="small">mo<\/td>/);
assert.match(el.html, /<dt>Chats<\/dt><dd class="small">Until the person deletes them\.<\/dd>/);
assert.match(el.html, /mo<\/td><td class="small">dev@magnus<\/td><td class="small"><span class="chip">Open until/);
assert.match(el.html, /Shop<\/td><td class="small">dev2@magnus<\/td><td class="small">Ended/);

// Saving the sign-in policy sends every field, keeping what the form does not show.
const form = ctx.$("[data-sign-in]");
await form.onsubmit({ preventDefault() {}, target: { fields: { code_required: "on", idle_minutes: "45", max_hours: "8", lockout_failures: "3", lockout_wait_minutes: "2", lockout_max_wait_minutes: "30" } } });
same(ctx.calls.find(c => c[0] === "PUT")[2], { code_required: true, lockout_enabled: false, lockout_permanent: true, idle_minutes: 45, max_hours: 8, lockout_failures: 3, lockout_wait_minutes: 2, lockout_max_wait_minutes: 30 });
assert.ok(ctx.toasts.includes("Sign-in rules saved."));

// The filter, then an export of what is filtered.
await ctx.$("[data-audit-filter]").onsubmit({ preventDefault() {}, target: { fields: { actor: "sam", action: "" } } });
assert.match(ctx.$("[data-audit]").html, /Nothing recorded yet\./);
vm.runInContext("securitySave = (f, c) => { globalThis.saved = [f, c]; };", ctx);
await ctx.$("[data-audit-export]").onclick();
assert.ok(ctx.calls.some(c => c[1] === "/v1/admin/audit/export?actor=sam"));
same(ctx.saved, ["audit.csv", "seq,at\n"]);
console.log("admin security check passed");
