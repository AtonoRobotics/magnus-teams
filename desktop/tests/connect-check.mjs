// Connecting an app shows one screen, what Magnus can read, change and
// send and the risks, and accepting it is the consent; a third-party
// server's new tools that change or send come back as one card for the app
// (spec rev 140; decisions/2026-11-10-connect-consent.md). No tool is
// reviewed on its own and no description is shown, as connect.js and
// app.js draw them.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
const sandbox = {
  esc, enc: encodeURIComponent,
  buttons: (i, list) => list.map(([d, label]) => `<button data-item="${i.id}" data-decision="${d}">${esc(label)}</button>`).join(""),
  api: async (method, path, body) => {
    calls.push([method, path, body]);
    return method === "GET" ? terms : body;
  },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("connect.js"), sandbox);

const terms = {
  app: "crm", title: "Crm", terms: "abc123",
  reads: ["It can: list deals."], changes: ["Changes that can be undone: update deal.", "Changes and sends that cannot be undone: send quote."],
  risks: ["What Magnus sends through Crm can reach anyone, outside your organisation too.", "Alpha Vector has not reviewed Crm."], cannot: null,
};
const screen = sandbox.connectScreenHtml(terms);
assert.match(screen, /What Magnus can read[\s\S]*list deals/, "the screen does not say what Magnus can read");
assert.match(screen, /What it can change and send[\s\S]*update deal[\s\S]*send quote/, "the screen does not say what it can change and send");
assert.match(screen, /The risks[\s\S]*can reach anyone/, "the screen does not say the risks");
assert.match(screen, /data-connect-answer="yes">Connect</, "the screen cannot be accepted");
assert.doesNotMatch(screen, /type="checkbox"|<select|data-tool=/, "the screen asks about tools one by one");
assert.match(sandbox.connectScreenHtml({ ...terms, changes: [] }), /Nothing: it only reads\./);

// Accepting records the consent with the terms that were shown; going back records nothing.
sandbox.connectScreen = async () => true;
assert.equal(await sandbox.consentToConnect("crm"), true);
assert.deepEqual(calls.map(([m, p]) => `${m} ${p}`), ["GET /v1/connectors/crm/terms", "POST /v1/connectors/crm/consent"]);
assert.equal(JSON.stringify(calls[1][2]), JSON.stringify({ terms: "abc123" }),"the consent does not name the terms shown");
calls.length = 0;
sandbox.connectScreen = async () => false;
assert.equal(await sandbox.consentToConnect("crm"), false);
assert.ok(!calls.some(([m]) => m === "POST"), "going back recorded a consent");

// One card per app: the new tools in plain words, keep or disconnect.
const item = { id: "i1", kind: "app_tools", can_respond: true, details: { app: "crm", title: "Crm", tools: [{ tool: "send_quote", risk: "irreversible", does: "send quote" }, { tool: "void_quote", risk: "irreversible", does: "void quote" }] } };
const card = sandbox.appToolsCard(item);
assert.match(card, /send quote[\s\S]*void quote/, "the card does not say what the new tools do");
assert.match(card, /data-decision="keep">Keep it</, "the card cannot keep them");
assert.match(card, /data-decision="disconnect">Disconnect Crm</, "the card cannot disconnect the app");
assert.equal((card.match(/data-decision=/g) || []).length, 2, "the card asks more than keep or disconnect");

// app.js draws the card, asks for it in Needs you, shows the screen before
// any sign-in, and has no per-tool review left. The page loads connect.js.
const app = ui("app.js");
assert.match(app, /case "app_tools":\s*body = appToolsCard\(i\);/, "the inbox does not draw the per-app card");
assert.match(app, /"app_tools"/, "the per-app card is not a decision");
assert.match(app, /if \(!again && !await consentToConnect\(name\)\) return;[\s\S]*\/sign-in`/, "connecting signs in without the connect screen");
for (const gone of ["tool_review", "tool_change", "Review the tool descriptions", "What the app says it does", "/tools/"]) {
  assert.ok(!app.includes(gone), `app.js still has the per-tool review: ${gone}`);
}
assert.match(ui("index.html"), /<script src="connect\.js"><\/script>/, "the page does not load connect.js");
console.log("connect ok");
