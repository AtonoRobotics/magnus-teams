// Fast-tier tests for Tier 2 Phase 2 (Actionable Morning Briefing, Inline Trust Graduation, Rule Capability Picker):
// 1. Briefing renders inline pending approval cards + batch Allow all N, Trust Graduation cards,
//    24h completed tasks with verified receipt count, and open commitments with Mark done.
// 2. app.js respond() surfaces an inline Trust Graduation toast action after repeated approvals.
// 3. rules.js ruleForm renders human-readable #rule-actions-list capability presets.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

// 1. Actionable Morning Briefing sections.
{
  let painted = "";
  const nowIso = new Date().toISOString();
  const main = { onclick: null };
  const ctx = {
    esc,
    enc: encodeURIComponent,
    App: {
      thread: null,
      schedules: [{ id: "s1", title: "Morning digest", daily_at: "08:00" }],
      tasks: [{ task_id: "t1", title: "Board brief", status: "completed", summary: "Ready for review", receipts_count: 3, ended_at: nowIso }],
    },
    Panel: { what: null },
    ENDED: ["completed", "failed", "cancelled"],
    needsMe: () => [
      { id: "i1", kind: "approval", can_respond: true, ask: { sentence: "Send email to Bob.", offers: ["allow", "dont_allow"] }, details: { tool: "gmail__send" } },
      { id: "i2", kind: "approval", can_respond: true, ask: { sentence: "Send email to Ada.", offers: ["allow", "dont_allow"] }, details: { tool: "gmail__send" } },
    ],
    batchableApprovals: list => list.map(i => i.id),
    askCard: i => `<div class="ask" data-ask="${i.id}">${i.ask.sentence}</div>`,
    paint: (_el, h) => { painted = h; },
    toast: () => {},
    Server: {
      call: async () => ({ stats: { completed_24h: 1, active_count: 0, staged_count: 0, overdue_count: 1 }, upcoming_schedules: [] }),
      spend: { get: async () => ({ spent_usd: 4.2, limit_usd: 20 }) },
      memory: { commitments: async () => ({ commitments: [{ id: "c1", status: "open", what: "Send Q4 deck", to: "Investors", by: "2026-11-28" }] }) },
      rules: { suggestions: async () => ({ suggestions: [{ connector: "gmail", tool: "gmail__send", destination: "@acme.com", count: 4, words: "Allow emails to @acme.com" }] }) },
    },
  };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("nova-pages.js") + "\nglobalThis.__NP = NovaPages;", ctx);
  await ctx.__NP.showBriefing(main);
  assert.match(painted, /data-brief-approvals/, "Briefing must render inline approvals section");
  assert.match(painted, /data-brief-batch>Allow all 2<\/button>/, "Briefing must render batch Allow all button");
  assert.match(painted, /data-brief-trust/, "Briefing must render Trust Graduation suggestions");
  assert.match(painted, /data-brief-graduate="0"/, "Briefing must render 1-click Allow automatically button");
  assert.match(painted, /data-brief-completed/, "Briefing must render Completed in the last 24 hours section");
  assert.match(painted, /3 verified/, "Briefing must show verified receipt count on completed tasks");
  assert.match(painted, /data-brief-commitments/, "Briefing must render Open commitments section");
  assert.match(painted, /data-brief-cmt-done="c1"/, "Briefing must render Mark done button for commitments");
  assert.match(ui("nova-pages.js"), /data-dry-preview/, "Briefing Dry run must render inline preview panel");
}

// 2. Inline Trust Graduation after repeated approvals in app.js.
{
  const appSrc = ui("app.js");
  assert.match(appSrc, /Server\.rules\?\.suggestions[\s\S]*label:\s*"Allow automatically"/, "respond() must offer inline Trust Graduation toast action after approval");
}

// 3. Human-readable action capability picker in rules.js.
{
  const ctx = { esc, App: { connectors: [{ name: "gmail", title: "Gmail", state: "connected" }] } };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("rules.js") + "\nglobalThis.__form = ruleForm;", ctx);
  const html = ctx.__form(null);
  assert.match(html, /list="rule-actions-list"/, "ruleForm Action input must link to rule-actions-list datalist");
  assert.match(html, /Send email \(Gmail\)/, "rule-actions-list must include human-readable action labels");
}

console.log("tier 2 phase 2 check ok");
