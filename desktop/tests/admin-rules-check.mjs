// Admin › Rules (spec rev 141): workspace rules that cap everyone's own,
// the Auto Review switch for the workspace, and the Cedar policy folded
// below for the admin who writes it.
import assert from "node:assert/strict";
import { wired, load, draw, node, refused, same } from "./admin-harness.mjs";

wired("admin-rules.js", "admin-rules-check.mjs");

const rules = [
  { id: "r1", rule: { effect: "block", connector: "gmail", tool: "send", destinations: ["*@rival.com"], words: "Never mail a rival", by: "admin:sam" } },
  { id: "r2", rule: { effect: "ask", connector: "shopify", tool: "", destinations: [], words: null, by: "admin:sam" } },
];
const ctx = load("admin-rules.js", {
  "GET /v1/admin/rules": { rules, auto_review: { acts: false, set_here: true, config: true } },
  "POST /v1/admin/rules": b => b.connector === "full" ? Promise.reject(refused(409, { error: "too_many", message: "A workspace has at most 200 rules." })) : { id: "r3" },
  "GET /v1/admin/policy": { sets: [{ id: "org", live: "permit(principal, action, resource);", draft: null }] },
  "GET /v1/admin/policy/history?layer=org": { versions: [{ version: 1, by: "admin:sam", at: null }, { version: 2, by: "admin:lee", at: null }] },
});
const el = await draw(ctx, "rules");
assert.match(el.html, /<span class="chip bad">Block: send in gmail to \*@rival\.com<\/span> <span class="small">"Never mail a rival"<\/span>/);
assert.match(el.html, /<span class="chip">Ask first: anything in shopify<\/span>/);
assert.match(el.html, /A block or ask first here wins over anyone's own allow/);
assert.doesNotMatch(el.html, /data-auto-review-acts checked/, "Auto Review shows on when the workspace turned it off");
assert.doesNotMatch(el.html, /as the server's config says/, "a setting made here is called the config's");

// Auto Review.
const box = ctx.$("[data-auto-review-acts]");
await box.onchange({ target: { checked: true } });
same(ctx.calls.find(c => c[0] === "PUT"), ["PUT", "/v1/admin/auto-review", { acts: true }]);

// A new rule, destinations split.
const form = ctx.$("[data-rule-new]");
await form.onsubmit({ preventDefault() {}, target: { fields: { effect: "block", connector: "slack", tool: "post", destinations: "#general, ,#ops", words: "Not in public" } } });
same(ctx.calls.find(c => c[0] === "POST" && c[1] === "/v1/admin/rules")[2], { effect: "block", connector: "slack", tool: "post", destinations: ["#general", "#ops"], words: "Not in public" });
assert.ok(ctx.toasts.includes("Rule added."));

// The cap's refusal is said.
await form.onsubmit({ preventDefault() {}, target: { fields: { effect: "allow", connector: "full" } } });
assert.ok(ctx.toasts.includes("A workspace has at most 200 rules."), ctx.toasts.join(" | "));

// Remove.
await el.onclick({ target: node({ "data-rule-remove": "" }, node({ "data-rule": "r2" })) });
assert.ok(ctx.calls.some(c => c[0] === "DELETE" && c[1] === "/v1/admin/rules/r2"));

// The policy loads only when opened, with the newest version first.
assert.ok(!ctx.calls.some(c => c[1] === "/v1/admin/policy"));
await ctx.$("[data-policy]").listeners.toggle({ target: { open: true } });
await new Promise(r => setImmediate(r));
const policy = ctx.$("[data-policy-body]").html;
assert.match(policy, /permit\(principal, action, resource\);/);
assert.ok(policy.indexOf("Version 2") < policy.indexOf("Version 1"), "the history is not newest first");
console.log("admin rules check passed");
