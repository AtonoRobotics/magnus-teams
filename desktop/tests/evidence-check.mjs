// A result's evidence (spec rev 147, Evidence comes from the record): what
// was done with what the server read back, the sources, and what was not
// done, as evidence.js draws it. A read-back that did not match never reads
// as done; text from the record is escaped.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const sandbox = { esc, when: t => String(t) };
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("evidence.js"), sandbox);

const sent = {
  call_id: "c1", tool: "mail__send", what: "Send in Mail, to bob@corp.example", to: ["bob@corp.example"], at: "2026-11-12T10:00:00Z",
  read_back: { call_id: "c1", tool: "mail__send", read: "get_message", found: { subject: "Q3" }, matches: true, note: "Read back with get_message: it matches what was done." },
};
const html = sandbox.evidenceSection({ evidence: {
  receipts: [sent],
  sources: [{ claim: "Revenue was <b>41.7M</b>", call_id: "c0", tool: "files__read", at: "q3.xlsx" }],
  not_done: [{ call_id: "c2", tool: "crm__update", status: "read_back_differs", what: "it does not match (status differs)" }],
} });
assert.match(html, /data-receipt="c1"/, "the receipt is not shown");
assert.match(html, /data-read-back="matches"/, "the read-back is not shown with the receipt");
assert.match(html, /data-sources/, "the sources are not shown");
assert.ok(!html.includes("<b>41.7M</b>"), "a claim's text was not escaped");
assert.match(html, /data-not-done[\s\S]*Did not read back as done/, "a read-back that differs reads as done");

assert.equal(sandbox.evidenceSection({}), "", "a task with no evidence shows something");
assert.equal(sandbox.evidenceSection({ evidence: { receipts: [], sources: [], not_done: [] } }), "");

assert.match(ui("index.html"), /<script src="evidence\.js"><\/script>/, "the page does not load evidence.js");
console.log("evidence-check: ok");
