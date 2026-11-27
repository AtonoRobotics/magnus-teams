// Fast-tier tests for Tier 2 Phase 3 (Visual Task Replay):
// 1. frameForStep picks the latest frame at or before the active step.
// 2. renderStage renders VM screenshot, plain-language tool call, decision chip, and output.
// 3. index.html, assets.rs, and nova-ux.js register and delegate to TaskReplay.
// 4. crates/magnus/src/api/work.rs timeline_of exposes frames when unredacted and omits them when redact_private is true.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const workRs = readFileSync(join(here, "../../crates/magnus/src/api/work.rs"), "utf8");
const assetsRs = readFileSync(join(here, "../../crates/magnus/src/ui/assets.rs"), "utf8");

const ctx = {};
ctx.globalThis = ctx;
vm.runInNewContext(ui("task-replay.js") + "\nglobalThis.__TR = TaskReplay;", ctx);
const TR = ctx.__TR;

// 1. frameForStep selects latest frame <= stepNum.
const frames = [
  { step: 1, image_b64: "AAA" },
  { step: 3, image_b64: "BBB" },
  { step: 6, image_b64: "CCC" },
];
assert.equal(TR.frameForStep(frames, 1)?.image_b64, "AAA");
assert.equal(TR.frameForStep(frames, 2)?.image_b64, "AAA");
assert.equal(TR.frameForStep(frames, 5)?.image_b64, "BBB");
assert.equal(TR.frameForStep(frames, 6)?.image_b64, "CCC");
assert.equal(TR.frameForStep([], 2), null);

// 2. renderStage renders screenshot, tool name, decision chip, and output.
const html = TR.renderStage(
  {
    step: 3,
    text: "Checking vendor pricing",
    calls: [{ tool: "browser__open", decisions: [{ stage: "policy", outcome: "allow" }], result: "Loaded page", is_error: false }],
  },
  { step: 3, image_b64: "BBB" },
);
assert.match(html, /data:image\/png;base64,BBB/, "renderStage must render the step screenshot");
assert.match(html, /open/, "renderStage must render plain tool name");
assert.match(html, /chip ok">allow<\/span>/, "renderStage must render boundary decision chip");
assert.match(html, /Loaded page/, "renderStage must render tool result output");

// 3. Registration & backend frames wiring.
assert.match(ui("index.html"), /<script src="task-replay\.js"><\/script>/, "index.html must load task-replay.js");
assert.match(assetsRs, /"task-replay\.js"/, "assets.rs must serve task-replay.js");
assert.match(ui("nova-ux.js"), /TaskReplay\.mount\(bar, steps\)/, "nova-ux.js must delegate to TaskReplay.mount");
assert.match(workRs, /let frames: Vec<Value> = if redact_private \{[\s\S]*?"frames": frames/, "timeline_of must include frames when unredacted and omit when redact_private");

console.log("task replay check ok");
