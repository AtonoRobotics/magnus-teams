// Settings, Data and privacy, "What Magnus may do on its own": beside the
// grants, the person's accepted rules that allow (Allow always among them),
// each removed in one tap, after which Magnus asks again. Rules that ask or
// block, words alone and suggestions not yet accepted are not listed
// (decisions/2026-11-19-handle-things-like-this-on-an-ask.md).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

let rules = [
  { id: "rule_a", person: "alice", words: "Always allow: Magnus wants to update issue in Linear.", compiled: { connector: "linear", tool: "linear__update_issue", destinations: [], once: false, effect: "allow" }, accepted: true, used: 2 },
  { id: "rule_b", person: "alice", words: null, compiled: { connector: "mail", tool: "mail__send", destinations: ["bob@corp.example"], once: false, effect: "allow" }, accepted: true, used: 0 },
  { id: "rule_ask", person: "alice", words: "Ask me before Slack", compiled: { connector: "slack", tool: "", destinations: [], once: false, effect: "ask" }, accepted: true, used: 0 },
  { id: "rule_words", person: "alice", words: "Be brief", compiled: null, accepted: true, used: 0 },
  { id: "rule_suggested", person: "alice", words: "Allow drafts", compiled: { connector: "mail", tool: "mail__draft", destinations: ["x@y.example"], once: false, effect: "allow" }, accepted: false, used: 0 },
];
const calls = [];
const toasts = [];
const Server = {
  rules: {
    list: async () => ({ rules }),
    remove: async id => { calls.push(["DELETE", id]); rules = rules.filter(r => r.id !== id); },
  },
};
const sandbox = { esc, Server, toast: m => toasts.push(m), paint: (el, html) => { el.html = html; } };
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("rules.js"), sandbox);

const el = { html: "" };
await sandbox.allowRules(el);
const listed = [...el.html.matchAll(/data-allow-rule="([^"]+)"/g)].map(m => m[1]);
assert.deepEqual(listed, ["rule_a", "rule_b"], "the list is not the person's accepted rules that allow");
assert.match(el.html, /<strong>Always allow: Magnus wants to update issue in Linear\.<\/strong>/, "a rule's words are not shown");
assert.match(el.html, /update issue in linear · used 2 times/, "what the rule matches is not said");
assert.match(el.html, /<strong>Always allow<\/strong>[\s\S]*send in mail to bob@corp\.example/, "a rule without words is not described");
assert.equal((el.html.match(/<button data-allow-rule-remove>Remove<\/button>/g) || []).length, 2, "each rule has no Remove");

// One tap removes it: no confirmation, and the list is drawn again.
const tap = id => ({ target: { closest: sel => (sel === "[data-allow-rule-remove]" ? { disabled: false, closest: () => ({ dataset: { allowRule: id } }) } : null) } });
await el.onclick(tap("rule_a"));
assert.deepEqual(calls, [["DELETE", "rule_a"]], "Remove did not remove the rule");
assert.ok(toasts.includes("Removed. Magnus asks you again next time."), toasts.join(" | "));
assert.doesNotMatch(el.html, /rule_a/, "the removed rule is still listed");
await el.onclick({ target: { closest: () => null } });
assert.equal(calls.length, 1, "a click elsewhere removed something");

rules = [];
await sandbox.allowRules(el);
assert.match(el.html, /None yet\. Choose "Allow always"/);

// Settings shows it beside the grants, and the page loads it.
const app = ui("app.js");
const data = app.match(/async function dataSection\(el\) \{[\s\S]*?\n\}\n/);
assert.ok(data, "app.js has no Data and privacy section");
assert.match(data[0], /<h3>What Magnus may do on its own<\/h3>[\s\S]*data-grants[\s\S]*<div data-allow-rules><\/div>[\s\S]*<h3>What you and Magnus said/, "the rules are not listed beside the grants");
assert.match(data[0], /await allowRules\(\$\("\[data-allow-rules\]", el\)\);/, "Settings does not draw the rules");
assert.match(ui("index.html"), /<script src="rules\.js"><\/script>/, "the page does not load rules.js");
console.log("allow rules check passed");
