// An ask's card (spec rev 140, How an ask looks): one plain sentence, the
// exact content one tap away, Allow, Allow for this task, Allow always and
// Don't allow, and none
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
// The grant form is app.js's own, as the page has it; the form a card holds
// is read as the page reads it, here from what the test put in it.
const appSource = ui("app.js");
const grantFormSource = appSource.match(/\nfunction grantForm\(k, f, apps\) \{[\s\S]*?\n\}\n/);
assert.ok(grantFormSource, "app.js has no grantForm to open in place");
const toasts = [];
const sandbox = { esc, toast: m => toasts.push(m), readGrantForm: root => root.form };
sandbox.globalThis = sandbox;
vm.runInNewContext(grantFormSource[0], sandbox);
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

const card = sandbox.askCard(item(["allow_once", "allow", "allow_always", "dont_allow"], "task"), true);
const sentence = card.match(/<p class="ask-sentence" data-ask-sentence>([^<]*)<\/p>/);
assert.ok(sentence, "the card has no sentence");
assert.equal(sentence[1], "Magnus wants to send in Mail, to eve@evil.example.");
assert.equal((sentence[1].match(/[.!?](\s|$)/g) || []).length, 1, "the card says more than one sentence");
assert.match(card, /data-conversation="thread-1"/, "the card does not name the conversation it belongs in");
const content = card.match(/<details class="ask-content" data-ask-content>([\s\S]*?)<\/details>/);
assert.ok(content, "the content is not one tap away");
for (const exact of ["eve@evil.example", "Q3", "Revenue was 41.7M"]) assert.ok(content[1].includes(exact), `the content lacks ${exact}`);
const buttons = [...card.matchAll(/<button[^>]*data-decision="([^"]+)"[^>]*>([^<]+)<\/button>/g)].map(m => [m[1], m[2]]);
assert.deepEqual(
  buttons,
  [["approve:once", "Allow"], ["approve:task", "Allow for this task"], ["approve:always", "Allow always"], ["reject", "Don&#39;t allow"]],
  "the answers are not Allow, Allow for this task, Allow always and Don't allow",
);
for (const hidden of ["person_rule", "rule_7", "Auto Review", "never named", "taint", "untrusted", "policy", "outside_request", "Why you are asked"]) {
  assert.ok(!card.includes(hidden), `the card shows ${hidden}`);
}

// A program's own ask: Allow always names the program it allows.
const program = item(["allow_once", "allow", "allow_always", "dont_allow"], "task");
program.ask.always_covers = "Blender";
assert.match(sandbox.askCard(program, true), /data-decision="approve:always"[^>]*>Allow Blender always</, "Allow always does not name the program");

// A payment: Allow is one action, and there is no Allow always.
const once = sandbox.askCard(item(["allow", "dont_allow"], "once"), true);
assert.deepEqual([...once.matchAll(/data-decision="([^"]+)"[^>]*>([^<]+)</g)].map(m => [m[1], m[2]]), [["approve:once", "Allow"], ["reject", "Don&#39;t allow"]]);
// Someone who cannot answer sees no buttons.
assert.doesNotMatch(sandbox.askCard(item(["allow", "dont_allow"], "task"), false), /<button/);
// A decided ask says what was decided, in plain words.
assert.match(sandbox.askResolved({ ...item([], "task"), resolution: { decision: "approve", scope: "task" } }), /Allowed for the rest of this work/);

// "Handle things like this" where Allow always is not offered: the grant
// form opens in place from the card's starting values, and its answer
// carries the grant (decisions/2026-11-19-handle-things-like-this-on-an-ask.md).
const handleItem = item(["allow_once", "allow", "handle", "dont_allow"], "task");
handleItem.ask.handle = { title: "Merge the release branch", apps: ["github"], app_title: "GitHub", allow: ["irreversible"], period: "week", max_usd: 50, your_apps: [{ name: "github", title: "GitHub" }, { name: "slack", title: "Slack" }] };
const handled = sandbox.askCard(handleItem, true);
const answers = handled.match(/<div class="row ask-answers">([\s\S]*?)<\/div>/)[1];
assert.doesNotMatch(answers, /handle/i, "Handle is one of the one-tap answers instead of opening the form");
const form = handled.match(/<details class="ask-handle" data-ask-handle="i1"[^>]*><summary>Handle things like this<\/summary>([\s\S]*?)<\/details>/);
assert.ok(form, "the card does not open the grant form in place");
assert.match(form[1], /data-grant-form/, "the card's Handle is not the grant form");
assert.match(form[1], /data-g="title" value="Merge the release branch"/, "the form does not start from the task's title");
assert.match(form[1], /data-g-app="github"[^>]*checked/, "the action's app is not checked");
assert.doesNotMatch(form[1], /data-g-app="slack"[^>]*checked/, "another app was checked");
assert.match(form[1], /data-g-allow="irreversible"[^>]*checked/, "what cannot be undone is not lifted");
assert.doesNotMatch(form[1], /data-g-allow="send"[^>]*checked/, "sending was lifted though the action sends nothing");
assert.match(form[1], /<button class="primary" data-item="i1" data-decision="approve:handle" data-offer="handle">Handle things like this<\/button>/, "the form has no Handle answer");
assert.doesNotMatch(sandbox.askCard(item(["allow_once", "allow", "dont_allow"], "task"), true), /ask-handle/, "Handle showed where it is not offered");
// Its answer carries the grant the form holds, or says what is missing.
const grant = { title: "Merge the release branch", apps: ["github"], allow: ["irreversible"], period: "week", max_usd: 50 };
const btn = f => ({ closest: sel => (sel === "[data-ask-handle]" ? { form: f } : null) });
const body = { decision: "approve", scope: "handle" };
assert.equal(sandbox.askHandle(btn(grant), body), true);
assert.deepEqual(body.grant, grant, "the answer does not carry the form's grant");
assert.equal(sandbox.askHandle(btn({ ...grant, apps: [] }), {}), false, "a grant with no app was sent");
assert.equal(toasts.at(-1), "Choose at least one app.");
assert.equal(sandbox.askHandle(btn({ ...grant, title: "" }), {}), false, "a grant with no title was sent");
assert.match(sandbox.askResolved({ ...item([], "task"), resolution: { decision: "approve", scope: "handle" } }), /Allowed for the rest of this work, and Magnus handles things like this on its own/);
// The page sends the form's grant with the card's Handle.
assert.match(appSource, /if \(scope === "handle" && !askHandle\(btn, body\)\) return;/, "app.js does not send the grant with Handle");

// app.js draws every approval with this card, with none of the old reasoning.
const app = ui("app.js");
assert.match(app, /case "approval":\s*\n\s*return `<article class="card item approval"[^\n]*askCard\(i, i\.can_respond\)/, "app.js does not draw approvals with askCard");
for (const gone of ["Why you are asked", "Approve once", "Approve for an hour", "approve:standing", "approve:window"]) {
  assert.ok(!app.includes(gone), `app.js still has ${gone}`);
}
assert.match(ui("index.html"), /<script src="asks\.js"><\/script>/, "the page does not load asks.js");
const spendCard = sandbox.askCard({ ...item(["allow", "dont_allow"], "once"), tier: "financial", details: { input: { supplier: "Apex", amount: 418 } } }, true);
assert.match(spendCard, /Spend guardrail:.*\$418.*Vault-only credentials/);
console.log("asks card ok");
