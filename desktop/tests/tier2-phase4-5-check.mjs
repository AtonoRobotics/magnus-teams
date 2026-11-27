// Regression tests for Tier 2 Phases 4 & 5:
//   - Per-Session Autonomy Leash selector in modelPicker() + chosenAutonomy()
//   - Strictly-tightening leash handling in personal-chat.js (cautious, draft_only, plan_first)
//   - Editable Pre-Execution Plan Surface (plan-surface.js) parsing, gate detection,
//     formatting, and registration in index.html + crates/magnus/src/ui/assets.rs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. Load plan-surface.js in a clean VM and verify pure helpers
const planSrc = read("desktop/ui/plan-surface.js");
const ctx = vm.createContext({});
vm.runInContext(`${planSrc}\nthis.__PlanSurface = PlanSurface;`, ctx);
const PlanSurface = ctx.__PlanSurface;
assert.ok(PlanSurface, "PlanSurface should be defined");

// parseSteps: numbered/bulleted lines
assert.deepEqual(
  Array.from(PlanSurface.parseSteps("1. Read the quarterly report\n2. Draft summary email\n3. Send email to finance")),
  ["Read the quarterly report", "Draft summary email", "Send email to finance"],
  "parseSteps should strip numbered list markers and preserve lines",
);
assert.deepEqual(
  Array.from(PlanSurface.parseSteps("- Inspect logs\n- Delete stale cache")),
  ["Inspect logs", "Delete stale cache"],
  "parseSteps should strip bullet markers",
);

// parseSteps: multi-clause sentence
assert.deepEqual(
  Array.from(PlanSurface.parseSteps("Read the PR diff, then post a review summary")),
  ["Read the PR diff", "post a review summary"],
  "parseSteps should split multi-clause 'then' instructions",
);

// parseSteps: single short goal -> 3-step starter plan
const starter = Array.from(PlanSurface.parseSteps("Prepare Q4 board deck"));
assert.equal(starter.length, 3, "single-clause goal should expand to 3 editable starter steps");
assert.match(starter[0], /Prepare Q4 board deck/);
assert.deepEqual(Array.from(PlanSurface.parseSteps("   ")), [], "blank prompt should yield empty steps");

// isGated: external/financial/destructive verbs vs read-only
for (const gated of [
  "Send email to Alice",
  "Publish blog post",
  "Delete old invoices",
  "Purchase domain renewal",
  "Merge pull request #42",
  "Deploy to production",
]) {
  assert.equal(PlanSurface.isGated(gated), true, `expected gated=true for: ${gated}`);
}
for (const safe of [
  "Read the quarterly report",
  "Summarize meeting notes",
  "Compare vendor quotes",
  "Draft a reply for review",
]) {
  assert.equal(PlanSurface.isGated(safe), false, `expected gated=false for: ${safe}`);
}

// formatApprovedPlan: numbered list + filters blank entries
const formatted = PlanSurface.formatApprovedPlan("Handle inbox", [
  "Read unread messages",
  "   ",
  "Draft replies for review",
]);
assert.equal(
  formatted,
  "Handle inbox\n\nFollow this approved plan in order:\n1. Read unread messages\n2. Draft replies for review",
);

// open() without document resolves directly to formatted plan
const noDomResult = await PlanSurface.open("Check metrics, then email summary");
assert.match(noDomResult, /Follow this approved plan in order:/);
assert.match(noDomResult, /1\. Check metrics/);
assert.match(noDomResult, /2\. email summary/);

// 2. Verify plan-surface.js modal wiring (edit, reorder, delete, add, gate chip, cancel/start)
assert.match(planSrc, /data-plan-step/, "plan surface renders editable step inputs");
assert.match(planSrc, /data-plan-gate/, "plan surface marks gated steps with Will ask chip");
assert.match(planSrc, /data-plan-up/, "plan surface supports moving steps up");
assert.match(planSrc, /data-plan-down/, "plan surface supports moving steps down");
assert.match(planSrc, /data-plan-del/, "plan surface supports removing steps");
assert.match(planSrc, /data-plan-add/, "plan surface supports adding steps");
assert.match(planSrc, /data-plan-start/, "plan surface has Start plan confirmation button");
assert.match(planSrc, /data-plan-cancel/, "plan surface has Cancel button");

// 3. Verify app.js Autonomy Leash selector + chosenAutonomy()
const appSrc = read("desktop/ui/app.js");
for (const mode of ["standard", "cautious", "draft_only", "plan_first"]) {
  assert.match(appSrc, new RegExp(`"${mode}"`), `app.js AUTONOMY_MODES should include ${mode}`);
}
assert.match(appSrc, /<select data-autonomy/, "modelPicker() should render <select data-autonomy>");
assert.match(appSrc, /function chosenAutonomy\s*\(root\)/, "app.js should define chosenAutonomy(root)");
assert.match(appSrc, /save\("autonomy-mode",\s*mode\)/, "chosenAutonomy() should persist autonomy-mode");

// 4. Verify personal-chat.js wires plan_first, cautious, draft_only, and autonomy_mode
const chatSrc = read("desktop/ui/personal-chat.js");
assert.match(chatSrc, /chosenAutonomy\(form\)/, "personal-chat.js should read chosenAutonomy(form)");
assert.match(chatSrc, /autonomy === "plan_first"[\s\S]*?PlanSurface\.open\(text\)/, "plan_first should open PlanSurface before sending");
assert.match(chatSrc, /if \(!approved\) return;/, "canceling PlanSurface should abort sending");
assert.match(chatSrc, /autonomy === "cautious"/, "personal-chat.js should apply cautious leash directive");
assert.match(chatSrc, /autonomy === "draft_only"/, "personal-chat.js should apply draft_only leash directive");
assert.match(chatSrc, /if \(autonomy !== "standard"\) body\.autonomy_mode = autonomy;/, "personal-chat.js should include autonomy_mode when non-standard");

// 5. Verify registration in index.html and crates/magnus/src/ui/assets.rs
const indexHtml = read("desktop/ui/index.html");
assert.match(indexHtml, /<script src="plan-surface\.js"><\/script>/, "index.html must load plan-surface.js");
const assetsRs = read("crates/magnus/src/ui/assets.rs");
assert.match(assetsRs, /plan-surface\.js/, "crates/magnus/src/ui/assets.rs must embed plan-surface.js");

console.log("tier2-phase4-5-check: ok");
