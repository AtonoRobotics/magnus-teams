// Regression tests for Tier 4 Phase 3:
//   - Live Command Palette items for pending approvals and recent/live tasks
//   - Verifiable receipt export (Markdown + JSON) in evidence.js
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. Test evidence.js pure export helpers in VM
const evSrc = read("desktop/ui/evidence.js");
const ctx = vm.createContext({
  esc: s => String(s ?? ""),
  when: s => String(s ?? ""),
  Date,
});
vm.runInContext(`${evSrc}\nthis.receiptsToMarkdown = receiptsToMarkdown; this.receiptsToJson = receiptsToJson; this.evidenceSection = evidenceSection;`, ctx);

const sampleReceipts = [
  { call_id: "c1", tool: "gmail__send", what: "Sent Q4 summary", to: ["cfo@acme.com"], at: "2026-11-27T10:00:00Z", read_back: { matches: true, note: "Verified in Sent folder" } },
];
const md = ctx.receiptsToMarkdown("Q4 Board Summary", sampleReceipts);
assert.match(md, /## Magnus Verification Proof — Q4 Board Summary/);
assert.match(md, /Sent Q4 summary/);
assert.match(md, /cfo@acme\.com/);
assert.match(md, /✓ Verified read-back/);

const json = JSON.parse(ctx.receiptsToJson("Q4 Board Summary", sampleReceipts));
assert.equal(json.title, "Q4 Board Summary");
assert.equal(json.receipts.length, 1);
assert.equal(json.receipts[0].call_id, "c1");

const html = ctx.evidenceSection({ task_id: "t-99", title: "Q4 Board Summary", evidence: { receipts: sampleReceipts, sources: [], not_done: [] } });
assert.match(html, /data-evidence-copy-md="t-99"/, "evidenceSection renders Copy proof button");
assert.match(html, /data-evidence-download-json="t-99"/, "evidenceSection renders Export JSON button");

// 2. Verify live approvals and tasks in Command Palette
const appSrc = read("desktop/ui/app.js");
assert.match(appSrc, /label: `Approval:/, "Command Palette includes pending approval entries");
assert.match(appSrc, /\(App\.tasks \|\| \[\]\)\.slice\(0,\s*12\)\.map/, "Command Palette includes live & recent task entries");

console.log("tier4-phase3-check: ok");
