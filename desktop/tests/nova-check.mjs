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

// 4. nova-pages.js renders Executive Morning Briefing & Quiet Routines with Run now / Dry run.
let painted = "";
const briefCtx = {
  esc, enc: encodeURIComponent,
  App: { thread: "t1", schedules: [{ id: "s1", title: "Morning digest", cadence: "Daily 08:00" }], tasks: [] },
  Panel: { what: null },
  ENDED: ["completed", "failed", "cancelled"],
  needsMe: () => [{ id: "i1" }],
  paint: (_el, h) => { painted = h; },
  Server: { call: async () => ({ summary: "All systems nominal." }) },
  toast: () => {},
};
briefCtx.globalThis = briefCtx;
vm.runInNewContext(ui("nova-pages.js") + "\nglobalThis.__NP = NovaPages;", briefCtx);
await briefCtx.__NP.showBriefing({});
assert.match(painted, /nova-briefing-page/, "briefing page should render");
assert.match(painted, /data-routine-run="s1"/, "briefing should render Run now button");
assert.match(painted, /data-routine-dry="s1"/, "briefing should render Dry run button");

// 5. Structural wiring in app.js, aurora-stage.js, and nova.css.
const appSrc = ui("app.js");
assert.match(appSrc, /route\(\/\^#\\\/briefing\$\//, "app.js must route #/briefing");
assert.match(appSrc, /nav-inbox-count/, "app.js nav must show live Inbox count badge");
assert.match(appSrc, /conv-more-menu/, "app.js convHead must tuck move/delete into overflow menu");

const stageSrc = ui("aurora-stage.js");
assert.match(stageSrc, /data-stage-batch-approve/, "aurora-stage.js must support 1-click batch approval");
assert.match(stageSrc, /b\.dataset\.tier\s*=/, "aurora-stage.js must set data-tier on stage tiles");

const novaCss = ui("nova.css");
assert.match(novaCss, /section\.conv:has\(\.hello-hero\)/, "nova.css must center empty Home intro");
assert.match(novaCss, /html\[data-style="nova"\] \.lgrid/, "nova.css must style 2-column Apps grid");
assert.match(novaCss, /html\[data-style="nova"\] \.explore-grid/, "nova.css must style 3-column Explore grid");
assert.match(novaCss, /\.stage-tile\[data-tier="financial"\]/, "nova.css must style Side Bento financial receipt tiles");

// 6. Complete theme isolation: no Aurora stylesheet may ever target Nova.
for (const sheet of ["aurora.css", "aurora-widgets.css", "aurora-stage.css", "aurora-rail.css", "aurora-compact.css"]) {
  assert.doesNotMatch(ui(sheet), /data-style="nova"/, `${sheet} must not style Nova`);
}
assert.doesNotMatch(ui("nova-stage.css"), /\.shell::before/, "nova-stage.css must not paint an opaque .shell::before overlay over main");

// 7. Nova needs nothing from an Aurora sheet: every animation a Nova sheet
// runs is a keyframe a Nova sheet (or base.css) defines, never an Aurora one.
const NOVA_SHEETS = ["nova.css", "nova-stage.css", "nova-shell.css", "nova-widgets.css"];
const novaAll = NOVA_SHEETS.map(ui).join("\n").replace(/\/\*[\s\S]*?\*\//g, "");
const frames = new Set([...(novaAll + ui("base.css")).matchAll(/@keyframes\s+([\w-]+)/g)].map(m => m[1]));
const auroraFrames = new Set(["aurora.css", "aurora-widgets.css", "aurora-stage.css", "aurora-rail.css", "aurora-compact.css"]
  .flatMap(s => [...ui(s).matchAll(/@keyframes\s+([\w-]+)/g)].map(m => m[1])));
for (const m of novaAll.matchAll(/animation(?:-name)?\s*:\s*([^;}]+)/g)) {
  const name = m[1].trim().split(/[\s,]+/)[0];
  if (name === "none") continue;
  assert.ok(!auroraFrames.has(name) || frames.has(name), `a Nova sheet runs Aurora's keyframes ${name}`);
  assert.ok(frames.has(name), `a Nova sheet runs keyframes ${name} that no Nova sheet defines`);
}
for (const f of frames) assert.ok(!auroraFrames.has(f), `Nova's keyframes ${f} share a name with Aurora's`);

// 8. The widgets the shared stage engines draw under Nova have Nova rules.
for (const c of ["aurora-reason", "aurora-reason-toggle", "aurora-reason-steps", "aurora-reason-chev", "aurora-reason-spark",
  "aurora-say-state", "aurora-say-text", "aurora-say-told", "aurora-state-dot", "stage-tile-acts", "stage-tile-stop",
  "stage-tile-link", "stage-tile-bar", "stage-preview-row", "stage-preview-key", "stage-preview-val", "stage-preview-body",
  "stage-bento-overflow", "aurora-confirmed-pill", "aurora-decision-line", "aurora-decision-dot", "aurora-decision-said",
  "aurora-decision-what", "aurora-decision-meta", "aurora-decision-open", "aurora-detail-ghost", "stage-step-dot"]) {
  assert.match(novaAll, new RegExp(`html\\[data-style="nova"\\][^{]*\\.${c}(?![\\w-])`), `Nova has no rule for .${c}`);
}

// 9. Light mode: the stage is --main (white), not the rail's grey.
assert.match(ui("theme.js"), /NOVA_LIGHT = \{[\s\S]*?main: "#FFFFFF"/, "Nova light --main is the white stage");
assert.match(ui("nova.css"), /html\[data-style="nova"\] body \{\s*background: var\(--main\);/, "Nova paints the stage with --main");
// The compact rail is see-through in both modes, as the mockup draws it
// (owner, 2026-11-26): in light it reads as the white stage.
assert.doesNotMatch(ui("nova.css"), /\[data-mode="light"\] body\[data-navigation=compact\] \.shell > nav\.side/, "no light-only fill on the compact rail");

// 10. New Chat: the composed two-column grid never applies to the intro.
const stageCss = ui("nova-stage.css");
const composed = [...stageCss.matchAll(/section\.conv\[data-stage-mode="composed"\][^{,]*/g)].map(m => m[0]);
assert.ok(composed.length > 0, "nova-stage.css composes the conversation with its boxes");
for (const sel of composed) assert.match(sel, /:not\(:has\(\.hello-hero\)\)/, `the composed grid must not apply to the New Chat intro: ${sel}`);

// 11. The right panel has one hairline: the aside's, not the artifact's too.
const artifactRule = stageCss.match(/html\[data-style="nova"\] \.stage-artifact \{[^}]*\}/)?.[0] || "";
assert.ok(artifactRule && !/border-left/.test(artifactRule), ".stage-artifact must not draw a second left border");

// 12. The rail is 60px and its inbox badge is amber, as in the mockup.
assert.match(ui("nova-shell.css"), /--nav-width: 60px/, "Nova's compact rail is a 60px column");
assert.doesNotMatch(novaAll, /(?:width|--nav-width)\s*:\s*64px/, "no Nova sheet hard-codes the shared 64px rail width");
assert.match(ui("nova.css"), /html\[data-style="nova"\] \.nav-inbox-count \{\s*background: var\(--warn\);/, "Nova's inbox badge is amber");

// 13. Apps & Skills rows are the mockup's .cat-row: tile, name over a
// one-line summary, then the action, on a 40px / text / auto grid.
assert.match(ui("nova.css"), /html\[data-style="nova"\] #lib-sections \.lcard \{\s*display: grid;\s*grid-template-columns: 40px minmax\(0, 1fr\) auto;/, "Nova's app rows are the mockup's catalog rows");

console.log("nova theme & overhaul check ok");
