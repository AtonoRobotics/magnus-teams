// Fast-tier tests for the greenfield Nova theme & UX overhaul:
// - Nova registers as a stage theme with Pip/Orb/Prism selectable companions;
// - nova-avatars.js registers both "pip" and "prism" on AuroraAvatar;
// - asks.js renders risk-tier attribute and progressive-disclosure decline drawer;
// - projects.js renders the quiet inline Project Hub composer and 2-column grid.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");

// 1. Nova avatars register "pip" and "prism" cleanly.
const defined = new Map();
const avatarCtx = {
  AuroraAvatar: { define: (name, make) => defined.set(name, make) },
  document: { documentElement: { dataset: { motion: "reduced" } }, addEventListener() {}, hidden: false },
  window: { devicePixelRatio: 1, addEventListener() {} },
  performance: { now: () => 0 },
  requestAnimationFrame: () => 0,
};
vm.runInNewContext(ui("nova-avatars.js"), avatarCtx);
assert.ok(defined.has("pip"), "nova-avatars.js must register pip");
assert.ok(defined.has("mochi"), "nova-avatars.js must register mochi");
assert.ok(defined.has("prism"), "nova-avatars.js must register prism");

// 2. asks.js includes data-tier and progressive-disclosure decline drawer.
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const askCtx = { esc };
askCtx.globalThis = askCtx;
vm.runInNewContext(ui("asks.js"), askCtx);
const card = askCtx.askCard({
  id: "a1", tier: "financial",
  ask: { sentence: "Pay invoice.", offers: ["allow", "dont_allow"], allow_scope: "once" },
  details: { input: { amount: 420 } },
}, true);
assert.match(card, /data-tier="financial"/, "askCard should carry data-tier");
assert.match(card, /<details class="ask-decline-wrap" data-ask-decline="a1">/, "decline note should use progressive disclosure");

// 3. projects.js renders inline composer and 2-column hub grid.
const projCtx = {
  esc, enc: encodeURIComponent, icon: () => "", attachButton: () => "", fileChip: () => "",
  taskRow: () => "", when: () => "now", until: () => "soon",
  document: { addEventListener() {} },
};
projCtx.globalThis = projCtx;
vm.runInNewContext(ui("projects.js") + "\nglobalThis.__P = Projects;", projCtx);
const html = projCtx.__P.pageHtml({ id: "p1", name: "Launch", instructions: "", files: [], threads: [], tasks: [], schedules: [] });
assert.match(html, /data-project-ask="p1"/, "project page should include inline composer");
assert.match(html, /project-hub-grid/, "project page should use 2-column hub grid");

console.log("nova theme & overhaul check ok");
