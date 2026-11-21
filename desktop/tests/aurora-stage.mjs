// Aurora's conversation and cards work as the mockup's agent demo
// (decisions/2026-11-20-aurora-stage.md), and its glass blurs what is behind
// it (decisions/2026-11-20-aurora-glass-blur.md). In a real browser, with the
// app's sheets and aurora-stage.js on a page shaped like the app's:
// - the conversation is the main card, with a row of cards under it, one for
//   each companion view with its icon;
// - the conversation drives the cards: work running lights Live Workspace
//   with the beam and what it is doing; a decision waiting puts Approvals
//   first, amber, and the cards slide to their new places;
// - work the person watches that finishes opens its result beside the
//   conversation on its own;
// - the canvas slides in, and the conversation and its cards narrow together
//   over time to share the stage about evenly; Close is the app's close;
// - a new block of an answer streams in word by word;
// - Aurora's islands blur what is behind them;
// - other styles have no cards.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

// What app.js gives the stage, cut to what it reads.
const stubs = `
var FX = { pending: [], opened: [] };
var ENDED = ["completed", "failed", "stopped", "cancelled"];
var Theme = { stage: s => ["aurora", "obsidian-aurora"].includes(s) };
var UI = { computer: '<rect x="3" y="4" width="18" height="12"/>' };
var App = { me: { id: "p1" }, thread: "c1", tasks: [], schedules: [{ id: "s1", title: "AI digest", next: "2026-11-27T20:09:00Z" }],
  connectors: [{ name: "gmail", title: "Gmail", state: "connected" }], computers: [{ id: "vm1", owner: { person: "p1" }, status: "running" }] };
var Panel = { what: null, autoOpened: false };
var Server = { files: { list: async () => ({ files: [{ id: "f1", name: "Report.pdf", at: "2026-11-20T10:00:00Z" }] }) } };
function icon(k) { return '<svg class="ic" data-icon="' + k + '"></svg>'; }
function when() { return "2m ago"; }
function until() { return "in 6 days"; }
function rule() { return "Every week"; }
function firstLine(s) { return String(s || "").split("\\n")[0]; }
function enc(s) { return encodeURIComponent(s); }
function stateOf(c) { return c.state; }
function needsYou(t) { return t.status === "suspended"; }
function needsMe() { return FX.pending; }
function controlWords(c) { return c.status === "running" ? { key: "ready", pill: "Ready" } : { key: "asleep", pill: "Asleep" }; }
function openPanel(what, o) { FX.opened.push(what + (o && o.auto ? " (auto)" : "")); }
function closePanel() {}
function drawPanel() {}`;

const page = `<!doctype html><html data-style="aurora" data-mode="dark" data-motion="full" data-background="off"
  style="--bg:#05070E;--surface:#0E1424;--main:#0B1020;--fg:#E8EEF9;--fg2:#B5C0D4;--muted:#8592A8;--faint:#6B7891;--line-strong:rgba(255,255,255,.2);--warn:#F5B544;--ok:#34D399;--shadow:0 20px 50px rgba(0,0,0,.4);--aurora-glass:rgba(13,19,34,.62);--aurora-glass-strong:rgba(15,22,40,.82);--aurora-rim:rgba(186,215,255,.12);--aurora-beam-a:#38BDF8;--aurora-beam-b:#9B6DFF;--aurora-spot:rgba(79,157,255,.2);--aurora-mx:50%;--aurora-my:50%;--aurora-stage-ms:520ms;--ease:cubic-bezier(.22,1,.36,1);--r-sm:10px;--r-md:16px;--r-lg:22px;--r-xl:26px;--r-pill:999px">
<head><style>${ui("base.css")}${ui("layout.css")}${ui("chat.css")}${ui("aurora.css")}${ui("aurora-widgets.css")}${ui("aurora-stage.css")}</style></head>
<body data-navigation="expanded" data-workspace="conversation"><div class="shell" style="--nav-width:228px;--panel-width:360px;--splitter-width:16px;--conversation-min-width:560px;--workspace-height:900px;height:900px"><div class="workspace-bar"></div>
  <nav class="side"><div class="brand"><span class="logo"><svg></svg></span></div></nav>
  <main id="main"><section class="conv"><header class="conv-head" id="conv-head"></header><div class="thread" id="thread"><div class="entry agent" data-entry="a1" data-kind="answer"><div class="answer-body"><p id="block">The trip is <strong>ready</strong> to book.</p><pre id="code">keep this</pre></div></div></div><div id="composer" class="composer-slot"><div class="say"><textarea id="msg"></textarea></div></div></section></main>
  <div id="workspace-splitter" hidden></div><aside class="detail" id="detail" hidden></aside>
</div><script>${stubs}</script><script>${ui("aurora-stage.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page);
  await p.evaluate(() => AuroraStage.refresh());
  await p.waitForTimeout(300);

  const tiles = () => p.evaluate(() => [...document.querySelectorAll("#main > .stage-tiles .stage-tile")].map(t => ({ key: t.dataset.tile, tone: t.dataset.tone, icon: t.querySelector(".stage-tile-ic svg")?.dataset.icon || "svg", open: t.dataset.open, desc: t.querySelector(".stage-tile-desc").textContent })));
  const first = await tiles();
  check("under the conversation is a card for every companion view", () => assert.deepEqual(first.map(t => t.key), ["live", "approvals", "next", "recent", "rules", "files", "apps", "computer"]));
  check("each card wears the icon of its view", () => assert.deepEqual(Object.fromEntries(first.map(t => [t.key, t.icon])), { live: "sparkles", approvals: "need", next: "calendar", recent: "clock", rules: "shield", files: "file", apps: "grid", computer: "svg" }));
  check("each card opens what it shows", () => assert.deepEqual(Object.fromEntries(first.map(t => [t.key, t.open])), { live: "tasks", approvals: "needs", next: "schedule:s1", recent: "tasks", rules: "rules", files: "file:f1", apps: "apps", computer: "computer:vm1" }));
  const layout = await p.evaluate(() => {
    const conv = document.querySelector("#main > section.conv").getBoundingClientRect(), row = document.querySelector("#main > .stage-tiles").getBoundingClientRect();
    return { under: row.top >= conv.bottom, sameColumn: Math.abs(row.left - conv.left) < 2 && Math.abs(row.width - conv.width) < 2, mainBg: getComputedStyle(document.getElementById("main")).backgroundColor, convRim: getComputedStyle(document.querySelector("#main > section.conv")).borderTopColor !== "rgba(0, 0, 0, 0)" };
  });
  check("the conversation is the main card and the cards sit under it in its column", () => assert.deepEqual(layout, { under: true, sameColumn: true, mainBg: "rgba(0, 0, 0, 0)", convRim: true }));

  // The conversation starts work: Live Workspace lights and says what it does.
  await p.evaluate(() => { App.tasks = [{ task_id: "t1", thread: "c1", title: "Plan the trip", status: "running", status_line: "Comparing flights" }]; return AuroraStage.refresh(); });
  await p.waitForTimeout(100);
  const live = await p.evaluate(() => { const t = document.querySelector('.stage-tile[data-tile="live"]'); return { tone: t.dataset.tone, beam: t.classList.contains("aurora-executing"), title: t.querySelector(".stage-tile-title").textContent, desc: t.querySelector(".stage-tile-desc").textContent }; });
  check("work running lights Live Workspace with the beam and what it is doing", () => assert.deepEqual(live, { tone: "run", beam: true, title: "Plan the trip", desc: "Comparing flights" }));

  // A decision arrives: Approvals goes first, amber, and the cards slide there.
  await p.evaluate(() => { document.getAnimations().forEach(a => a.cancel()); FX.pending = [{ id: "i1", summary: "Send the booking?" }]; return AuroraStage.refresh(); });
  const moving = await p.evaluate(() => document.getAnimations().filter(a => a.effect?.target?.classList?.contains("stage-tile") && JSON.stringify(a.effect.getKeyframes()).includes("translate")).map(a => a.effect.target.dataset.tile));
  const now = await tiles();
  const badge = await p.evaluate(() => document.querySelector('.stage-tile[data-tile="approvals"] .stage-tile-badge')?.textContent);
  check("a decision waiting puts Approvals first, amber, with its count", () => assert.deepEqual([now[0].key, now[0].tone, now[0].desc, badge], ["approvals", "needs", "Nothing goes out until you approve.", "1"]));
  check("the cards slide to their new places", () => assert.ok(moving.includes("approvals") && moving.includes("live"), JSON.stringify(moving)));

  // The work finishes: its result opens beside the conversation on its own.
  await p.evaluate(() => { FX.pending = []; App.tasks = [{ task_id: "t1", thread: "c1", title: "Plan the trip", status: "completed", summary: "3-day itinerary · $912 total" }]; return AuroraStage.refresh(); });
  const done = await p.evaluate(() => ({ opened: FX.opened, tone: document.querySelector('.stage-tile[data-tile="live"]').dataset.tone, desc: document.querySelector('.stage-tile[data-tile="live"] .stage-tile-desc').textContent }));
  check("finished work opens its result beside the conversation on its own", () => assert.deepEqual(done.opened, ["task:t1 (auto)"]));
  check("Live Workspace turns mint with the result", () => assert.deepEqual([done.tone, done.desc], ["done", "3-day itinerary · $912 total"]));

  // The canvas slides in; the conversation and its cards narrow together.
  const width = () => p.evaluate(() => Math.round(document.querySelector("#main > .stage-tiles").getBoundingClientRect().width));
  const wide = await width();
  const topBefore = await p.evaluate(() => Math.round(document.querySelector("#main > section.conv").getBoundingClientRect().top));
  await p.evaluate(() => {
    const el = document.getElementById("detail");
    el.innerHTML = AuroraStage.shell({ kind: "task", title: "Plan the trip", head: null, backBtn: "", body: "<p>The plan</p>" });
    AuroraStage.settle(el, true);
    el.hidden = false; document.getElementById("workspace-splitter").hidden = false; document.body.classList.add("with-detail"); document.body.dataset.workspace = "split";
  });
  const canvas = await p.evaluate(() => { const c = document.querySelector(".stage-artifact"); return { opening: c.classList.contains("stage-opening"), close: c.querySelector(".artifact-close").hasAttribute("data-toggle-inspector"), title: c.querySelector("h2").textContent }; });
  await p.waitForTimeout(150);
  const midway = await width();
  await p.waitForTimeout(900);
  const narrow = await width();
  const cols = await p.evaluate(() => getComputedStyle(document.querySelector(".shell")).gridTemplateColumns.split(" ").map(parseFloat));
  const convTop = await p.evaluate(() => Math.round(document.querySelector("#main > section.conv").getBoundingClientRect().top));
  check("the canvas slides in with the app's close", () => assert.deepEqual(canvas, { opening: true, close: true, title: "Plan the trip" }));
  check("the conversation and its cards narrow together over time", () => assert.ok(midway < wide && narrow < midway, JSON.stringify({ wide, midway, narrow })));
  check("the conversation stays where it was when the canvas opens", () => assert.equal(convTop, topBefore));
  check("the conversation and the canvas share the stage about evenly", () => assert.ok(Math.abs(cols[1] - cols[3]) < 0.1 * cols[1], JSON.stringify(cols)));

  // A new block of an answer streams in word by word; code stays as it is.
  const words = await p.evaluate(() => { AuroraStage.stream(document.querySelector(".answer-body")); return { words: [...document.querySelectorAll("#block .stream-word")].map(w => `${w.textContent}@${w.style.animationDelay}`), text: document.getElementById("block").textContent, code: document.querySelectorAll("#code .stream-word").length }; });
  check("a new block streams in word by word", () => assert.deepEqual(words, { words: ["The@0ms", "trip@34ms", "is@68ms", "ready@102ms", "to@136ms", "book.@170ms"], text: "The trip is ready to book.", code: 0 }));

  // Frosted glass.
  const blur = await p.evaluate(() => Object.fromEntries([["nav", "nav.side"], ["composer", ".say"], ["conversation", "#main > section.conv"], ["card", ".stage-tile"], ["canvas", ".stage-artifact"], ["column", "aside.detail"]].map(([k, q]) => [k, getComputedStyle(document.querySelector(q)).backdropFilter])));
  check("Aurora's islands blur what is behind them; the canvas's column does not", () => assert.deepEqual(blur, { nav: "blur(22px)", composer: "blur(22px)", conversation: "blur(20px)", card: "blur(20px)", canvas: "blur(20px)", column: "none" }));

  // Other styles have no cards.
  const plain = await p.evaluate(async () => { document.documentElement.dataset.style = "obsidian"; await AuroraStage.refresh(); return { on: AuroraStage.on(), tiles: !!document.querySelector(".stage-tiles") }; });
  check("another style has no cards", () => assert.deepEqual(plain, { on: false, tiles: false }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
