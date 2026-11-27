// Regression tests for Tier 5 Phases 1–3:
//   - Rich Visual Task Replay metadata (tool input summary, Input details, verified count)
//   - Native OS folder picker hook (Magnus.pickFolder + Browse folder button in computers.js)
//   - Outside-agent admission revoke (Server.outsideAgents.revoke + Stop letting it ask me)
//   - Inline live Plan Card tracking via UIStore in plan-surface.js + personal-chat.js
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. TaskReplay rich metadata
const replayCtx = vm.createContext({});
vm.runInContext(`${read("desktop/ui/task-replay.js")}\nthis.__TaskReplay = TaskReplay;`, replayCtx);
const TR = replayCtx.__TaskReplay;
assert.equal(TR.summarizeInput({ to: "cfo@acme.com", subject: "Q4", nested: { a: 1 } }), "to: cfo@acme.com · subject: Q4");
const stageHtml = TR.renderStage(
  { step: 1, calls: [{ tool: "gmail__send", input: { to: "cfo@acme.com", subject: "Q4" }, decisions: [{ outcome: "allow" }], result: "Sent" }] },
  null,
  [{ criterion: "email sent", passed: true }],
);
assert.match(stageHtml, /data-replay-meta/);
assert.match(stageHtml, /✓ 1 verified/);
assert.match(stageHtml, /data-replay-input/);
assert.match(stageHtml, /Input details/);

// 2. Folder picker + outside-agent admitted badge wiring
assert.match(read("desktop/ui/bridge.js"), /async pickFolder\(\)/);
assert.match(read("desktop/ui/computers.js"), /data-act="local-browse-folder"/);
assert.match(read("desktop/ui/computers.js"), /Magnus\.pickFolder\(\)/);
assert.match(read("desktop/ui/outside-agents.js"), /data-outside-allowed/);

// 3. Inline live Plan Card via UIStore
const planCtx = vm.createContext({ Date });
vm.runInContext(`${read("desktop/ui/ui-store.js")}\n${read("desktop/ui/plan-surface.js")}\nthis.__UIStore = UIStore; this.__PlanSurface = PlanSurface;`, planCtx);
planCtx.__PlanSurface.trackTask("task-77", ["Read inbox", "Draft replies"]);
planCtx.__UIStore.setTask({ id: "task-77", status: "running", plan: { units: [{ status: "done" }, { status: "active" }] } });
const liveHtml = planCtx.__PlanSurface.renderLiveCard("task-77");
assert.match(liveHtml, /data-live-plan="task-77"/);
assert.match(liveHtml, /data-plan-unit-state="done"[\s\S]*?Read inbox/);
assert.match(liveHtml, /data-plan-unit-state="active"[\s\S]*?Draft replies/);
assert.match(read("desktop/ui/personal-chat.js"), /PlanSurface\.trackTask\(startedId,\s*approvedSteps\)/);

console.log("tier5-phases1-3-check: ok");
