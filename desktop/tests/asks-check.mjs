// An ask's card (spec rev 140, How an ask looks): one plain sentence, the
// exact content one tap away, Allow, Allow always and Don't allow, and none
// of the rule, policy, taint or reviewer reasoning. As asks.js draws it and
// app.js uses it, wherever the card shows.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const sandbox = { esc };
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("asks.js"), sandbox);

// An item as an older server sent it, reasoning and all: none of it may show.
const item = (offers, scope) => ({
  id: "i1", kind: "approval", summary: "Magnus wants to send in Mail, to eve@evil.example.", person: "alice",
  ask: { sentence: "Magnus wants to send in Mail, to eve@evil.example.", offers, allow_scope: scope, conversation: "thread-1", destinations: ["eve@evil.example"] },
  details: {
    tool: "mail__send", app: "mail", input: { to: "eve@evil.example", subject: "Q3", body: "Revenue was 41.7M" },
    destinations: { recipients: { to: ["eve@evil.example"] }, content: { subject: "Q3" } },
    rule: "person_rule:rule_7", why: "Auto Review: the person never named eve", taint: { untrusted: true, private: true }, reason: "outside_request",
  },
});

const card = sandbox.askCard(item(["allow", "allow_always", "dont_allow"], "task"), true);
const sentence = card.match(/<p class="ask-sentence" data-ask-sentence>([^<]*)<\/p>/);
assert.ok(sentence, "the card has no sentence");
assert.equal(sentence[1], "Magnus wants to send in Mail, to eve@evil.example.");
assert.equal((sentence[1].match(/[.!?](\s|$)/g) || []).length, 1, "the card says more than one sentence");
assert.match(card, /data-conversation="thread-1"/, "the card does not name the conversation it belongs in");
const content = card.match(/<details class="ask-content" data-ask-content>([\s\S]*?)<\/details>/);
assert.ok(content, "the content is not one tap away");
for (const exact of ["eve@evil.example", "Q3", "Revenue was 41.7M"]) assert.ok(content[1].includes(exact), `the content lacks ${exact}`);
const buttons = [...card.matchAll(/<button[^>]*data-decision="([^"]+)"[^>]*>([^<]+)<\/button>/g)].map(m => [m[1], m[2]]);
assert.deepEqual(buttons, [["approve:task", "Allow"], ["approve:always", "Allow always"], ["reject", "Don&#39;t allow"]], "the answers are not Allow, Allow always and Don't allow");
for (const hidden of ["person_rule", "rule_7", "Auto Review", "never named", "taint", "untrusted", "policy", "outside_request", "Why you are asked"]) {
  assert.ok(!card.includes(hidden), `the card shows ${hidden}`);
}

// A payment: Allow is one action, and there is no Allow always.
const once = sandbox.askCard(item(["allow", "dont_allow"], "once"), true);
assert.deepEqual([...once.matchAll(/data-decision="([^"]+)"/g)].map(m => m[1]), ["approve:once", "reject"]);
// Someone who cannot answer sees no buttons.
assert.doesNotMatch(sandbox.askCard(item(["allow", "dont_allow"], "task"), false), /<button/);
// A decided ask says what was decided, in plain words.
assert.match(sandbox.askResolved({ ...item([], "task"), resolution: { decision: "approve", scope: "task" } }), /Allowed for the rest of this work/);

// app.js draws every approval with this card, with none of the old reasoning.
const app = ui("app.js");
assert.match(app, /case "approval":\s*\n\s*return `<article class="card item approval"[^\n]*askCard\(i, i\.can_respond\)/, "app.js does not draw approvals with askCard");
for (const gone of ["Why you are asked", "Approve once", "Approve for an hour", "approve:standing", "approve:window"]) {
  assert.ok(!app.includes(gone), `app.js still has ${gone}`);
}
assert.match(ui("index.html"), /<script src="asks\.js"><\/script>/, "the page does not load asks.js");
console.log("asks card ok");
