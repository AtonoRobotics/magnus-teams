// Admin › Connectors (spec rev 141): the allow-list, limiting to groups,
// workspace-wide connections, who connected what, the organisation's key,
// and suspending pointed at the kill switch rather than a second switch.
import assert from "node:assert/strict";
import vm from "node:vm";
import { wired, load, draw, node, same } from "./admin-harness.mjs";

wired("admin-connectors.js", "admin-connectors-check.mjs");

const connectors = [
  { name: "github", title: "GitHub", enabled: true, status: "ok", source: "catalog", sign_in: "oauth", access_groups: null, credential: { kind: "oauth", set: true },
    connected_by: [{ id: "mo", name: "Mo" }], workspaces: [{ id: "w1", name: "Shop", roles: ["member"] }], last_used: null },
  { name: "acme", title: "Acme", enabled: false, status: "ok", source: "added", sign_in: "key", access_groups: ["ops", "sales"], credential: { kind: "key", set: false },
    connected_by: [], workspaces: [], last_used: null },
];
const ctx = load("admin-connectors.js", { "GET /v1/admin/connectors": { connectors } });
const el = await draw(ctx, "connectors");
const row = n => el.html.split(`data-connector="${n}"`)[1].split("</tr>")[0];
assert.match(row("github"), /data-connector-allow checked/);
assert.match(row("github"), /Shop <span class="muted">\(member\)/, "the workspace connection is not shown");
assert.match(row("github"), /Mo/, "who connected it is not shown");
assert.doesNotMatch(row("acme"), /data-connector-allow checked/);
assert.match(row("acme"), /value="ops, sales"/);
assert.match(row("acme"), /Needs its credential/);
assert.match(row("acme"), /No one/);
assert.match(el.html, /use the kill switch above/, "suspending does not point at the kill switch");
assert.doesNotMatch(el.html, /data-connector-suspend/, "a second switch is kept here");

const acme = node({ "data-connector": "acme" });
const allow = node({ "data-connector-allow": "" }, acme);
allow.checked = true;
await el.onchange({ target: allow });
same(ctx.calls.find(c => c[0] === "PATCH"), ["PATCH", "/v1/admin/connectors/acme", { enabled: true }]);

await el.onchange({ target: node({ "data-connector-groups": "", value: " ops , ,finance" }, acme) });
same(ctx.calls.filter(c => c[0] === "PATCH")[1], ["PATCH", "/v1/admin/connectors/acme", { access_groups: ["ops", "finance"] }]);

vm.runInContext("connectorKeySheet = async () => 'sk-123';", ctx);
await el.onclick({ target: node({ "data-connector-key": "" }, acme) });
same(ctx.calls.find(c => c[0] === "PUT"), ["PUT", "/v1/admin/connectors/acme/credential", { value: "sk-123" }]);
assert.ok(ctx.toasts.includes("Key saved."));

// Cancelling the key sheet sends nothing.
vm.runInContext("connectorKeySheet = async () => null;", ctx);
const puts = ctx.calls.filter(c => c[0] === "PUT").length;
await el.onclick({ target: node({ "data-connector-key": "" }, acme) });
assert.equal(ctx.calls.filter(c => c[0] === "PUT").length, puts);
console.log("admin connectors check passed");
