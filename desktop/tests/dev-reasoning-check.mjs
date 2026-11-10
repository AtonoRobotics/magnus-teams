// What users never see goes to the developer view and the run inspector
// (spec rev 140, What users and developers see): which ask it was, the rule,
// the reason Auto Review or the kernel gave, and the run's taint; and each
// boundary decision with its reason and rule.
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

const why = sandbox.askReasoning({ reason: "new_destination", rule: "kernel:rule_of_two", why: "the work has read untrusted content and private data", taint: { untrusted: true, private: true } });
assert.match(why, /data-reasoning/);
assert.match(why, /Private data to a new destination after untrusted input/, "the ask is not named");
assert.match(why, /kernel:rule_of_two/, "the rule is not shown");
assert.match(why, /the work has read untrusted content and private data/, "the reason is not shown");
assert.match(why, /untrusted input and private data/, "the taint is not shown");
assert.equal(sandbox.askReasoning(null), "", "a row with no ask drew reasoning");

const rows = sandbox.decisionRows([{ stage: "auto_review", outcome: "ask", reason: "mallory was never named", rule: "auto_review" }]);
assert.match(rows, /auto_review: ask<\/span> · mallory was never named/, "a decision's reason is not shown");
assert.match(rows, /\(auto_review\)/, "a decision's rule is not shown");

// The run inspector draws both.
const dev = ui("dev.js");
assert.match(dev, /askReasoning\(x\.reasoning\)/, "the run inspector does not draw an ask's reasoning");
assert.match(dev, /decisionRows\(a\.decisions\)/, "the run inspector does not draw decisions' reasons");
console.log("developer reasoning ok");
