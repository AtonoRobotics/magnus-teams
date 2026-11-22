// Aurora's conversation and its bento work as the mockup's agent demo
// (decisions/2026-11-20-aurora-stage.md), and its glass blurs what is behind
// it (decisions/2026-11-20-aurora-glass-blur.md). In a real browser, with the
// app's sheets and aurora-stage.js on a page shaped like the app's:
// - with nothing relevant there are no boxes, and every view is still one
//   click away in the conversation's head, with the panel's icons;
// - boxes come as they become relevant and go when they stop: work starting
//   brings Live Workspace with the beam and what it is doing; a decision
//   brings Approvals first, amber, and the others slide over; the decision
//   answered takes Approvals away; a routine coming due, a new file, bring
//   theirs;
// - work the person watches that finishes opens its result beside the
//   conversation on its own, and Live Workspace turns mint with the result;
// - the canvas slides in, and the conversation narrows over time to share
//   the stage about evenly, the conversation not moving
//   down; Close is the app's close;
// - a new block of an answer streams in word by word;
// - Aurora's islands blur what is behind them; the frame the conversation
//   sits in is denser glass, without the cost of a blur the size of the
//   window;
// - other styles have no bento and no dock.
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

// What app.js gives the stage, cut to what it reads. FRESH_FILE says whether
// the person's latest file is new.
const stubs = fresh => `
var DAY_MS = 864e5;
var FX = { pending: [], opened: [] };
var ENDED = ["completed", "failed", "stopped", "cancelled"];
var Theme = { stage: s => ["aurora", "obsidian-aurora"].includes(s) };
var UI = { computer: '<rect x="3" y="4" width="18" height="12"/>' };
var App = { me: { id: "p1" }, thread: "c1", tasks: [], schedules: [{ id: "s1", title: "AI digest", next: new Date(Date.now() + 7 * DAY_MS).toISOString() }],
  connectors: [{ name: "gmail", title: "Gmail", state: "connected" }], computers: [{ id: "vm1", owner: { person: "p1" }, status: "running" }] };
var Panel = { what: null, autoOpened: false };
var Server = { files: { list: async () => ({ files: [{ id: "f1", name: "Itinerary.pdf", at: new Date(Date.now() - (${fresh ? 60_000 : 3 * 864e5})).toISOString() }] }) } };
function icon(k) { return '<svg class="ic" data-icon="' + k + '"></svg>'; }
function when() { return "just now"; }
function until() { return "in 2 hours"; }
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

const page = fresh => `<!doctype html><html data-style="aurora" data-mode="dark" data-motion="full" data-background="off"
  style="--bg:#05070E;--surface:#0E1424;--main:#0B1020;--fg:#E8EEF9;--fg2:#B5C0D4;--muted:#8592A8;--faint:#6B7891;--line-strong:rgba(255,255,255,.2);--warn:#F5B544;--ok:#34D399;--shadow:0 20px 50px rgba(0,0,0,.4);--aurora-glass:rgba(13,19,34,.62);--aurora-glass-strong:rgba(15,22,40,.82);--aurora-rim:rgba(186,215,255,.12);--aurora-beam-a:#38BDF8;--aurora-beam-b:#9B6DFF;--aurora-spot:rgba(79,157,255,.2);--aurora-mx:50%;--aurora-my:50%;--aurora-stage-ms:520ms;--ease:cubic-bezier(.22,1,.36,1);--r-sm:10px;--r-md:16px;--r-lg:22px;--r-xl:26px;--r-pill:999px">
<head><style>${ui("base.css")}${ui("layout.css")}${ui("chat.css")}${ui("aurora.css")}${ui("aurora-widgets.css")}${ui("aurora-stage.css")}</style></head>
<body data-navigation="expanded" data-workspace="conversation"><div class="shell" style="--nav-width:228px;--panel-width:360px;--splitter-width:16px;--conversation-min-width:560px;--workspace-height:900px;height:900px"><div class="workspace-bar"></div>
  <nav class="side"><div class="brand"><span class="logo"><svg></svg></span></div></nav>
  <main id="main"><section class="conv"><header class="conv-head" id="conv-head"><div class="conv-title"><h1>Plan the trip</h1></div><div class="row"></div></header><div class="thread" id="thread"><div class="entry agent" data-entry="a1" data-kind="answer"><div class="answer-body"><p id="block">The trip is <strong>ready</strong> to book.</p><pre id="code">keep this</pre></div></div></div><div id="composer" class="composer-slot"><div class="say"><textarea id="msg"></textarea></div></div></section></main>
  <div id="workspace-splitter" hidden></div><aside class="detail" id="detail" hidden></aside>
</div><script>${stubs(fresh)}</script><script>${ui("aurora-boxes.js")}</script><script>${ui("aurora-stage.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page(false));
  const step = async fn => { await p.evaluate(fn); await p.evaluate(() => AuroraStage.refresh()); };
  const shown = () => p.evaluate(() => [...document.querySelectorAll("section.conv > .stage-bento > .stage-tile:not(.stage-tile-leaving)")].map(t => ({ key: t.dataset.tile, tone: t.dataset.tone, title: t.querySelector(".stage-tile-title").textContent, desc: t.querySelector(".stage-tile-desc").textContent, wide: getComputedStyle(t).gridColumnEnd === "-1", entering: t.classList.contains("stage-tile-enter"), beam: t.classList.contains("aurora-executing") })));
  await step(() => {});
  await p.waitForTimeout(200);

  const quiet = await shown();
  const fold = await p.evaluate(() => Math.round(document.querySelector("section.conv > .stage-bento").getBoundingClientRect().height));
  const dock = await p.evaluate(() => ({
    direct: [...document.querySelectorAll("#conv-head .row > .stage-dock > a")].map(a => `${a.dataset.open}:${a.querySelector("svg").dataset.icon}`),
    menu: [...document.querySelectorAll("#conv-head .row > .stage-dock .stage-dock-menu a")].map(a => `${a.dataset.open}:${a.querySelector("svg").dataset.icon}`),
  }));
  check("with nothing relevant there are no boxes and the bento folds away", () => assert.deepEqual([quiet, fold], [[], 0]));
  check("Approvals sits in the head and the other views live in the compact Views popover", () => assert.deepEqual(dock, { direct: ["needs:need"], menu: ["tasks:clock", "schedules:calendar", "rules:shield", "files:file", "apps:grid"] }));

  // Work starts in this conversation: Live Workspace comes in, lit.
  await step(() => { App.tasks = [{ task_id: "t1", thread: "c1", title: "Plan the trip", status: "running", status_line: "Comparing flights" }]; });
  const started = await shown();
  check("work starting brings Live Workspace in, with the beam and what it is doing", () => assert.deepEqual(started, [{ key: "live", tone: "run", title: "Plan the trip", desc: "Comparing flights", wide: true, entering: true, beam: true }]));
  await p.waitForTimeout(600);

  // A decision: Approvals comes first, amber; Live Workspace slides over.
  await p.evaluate(() => document.getAnimations().forEach(a => a.cancel()));
  await step(() => { FX.pending = [{ id: "i1", summary: "Send the booking?" }]; });
  const slid = await p.evaluate(() => document.getAnimations().filter(a => !(a instanceof CSSAnimation) && a.effect?.target?.dataset?.tile && JSON.stringify(a.effect.getKeyframes()).includes("translate")).map(a => a.effect.target.dataset.tile));
  const decided = await shown();
  const badge = await p.evaluate(() => document.querySelector('#conv-head .stage-dock a[data-open="needs"] .stage-dock-badge')?.textContent);
  check("a decision brings Approvals in first, amber, beside the work", () => assert.deepEqual(decided.map(t => `${t.key}:${t.tone}:${t.wide}`), ["approval:i1:needs:false", "live:run:false"]));
  check("the box already there slides to its new place", () => assert.deepEqual(slid, ["live"]));
  check("the dock counts what waits", () => assert.equal(badge, "1"));
  await p.waitForTimeout(600);

  // The decision answered: Approvals leaves from where it was.
  await step(() => { FX.pending = []; });
  const leaving = await p.evaluate(() => { const b = document.querySelector('.stage-tile[data-tile="approval:i1"]'); return b ? { leaving: b.classList.contains("stage-tile-leaving"), position: b.style.position } : null; });
  await p.waitForTimeout(500);
  const gone = await p.evaluate(() => !document.querySelector('.stage-tile[data-tile="approval:i1"]'));
  check("the decision answered takes Approvals away, leaving from where it was", () => assert.deepEqual([leaving, gone], [{ leaving: true, position: "absolute" }, true]));

  // A routine comes due: Up Next comes in.
  await step(() => { App.schedules[0].next = new Date(Date.now() + 2 * 3600e3).toISOString(); });
  const due = (await shown()).map(t => `${t.key}:${t.title}`);
  check("a routine coming due brings Up Next in", () => assert.deepEqual(due, ["live:Plan the trip", "next:AI digest"]));

  // The work finishes: its result opens on its own; Live Workspace turns mint.
  await step(() => { App.tasks = [{ task_id: "t1", thread: "c1", title: "Plan the trip", status: "completed", summary: "3-day itinerary · $912 total", ended_at: new Date().toISOString() }]; });
  const done = await p.evaluate(() => ({ opened: FX.opened, live: [...document.querySelectorAll('.stage-tile[data-tile="live"]')].map(t => `${t.dataset.tone}:${t.querySelector(".stage-tile-desc").textContent}`) }));
  check("finished work opens its result beside the conversation on its own", () => assert.deepEqual(done.opened, ["task:t1 (auto)"]));
  check("Live Workspace turns mint with the result", () => assert.deepEqual(done.live, ["done:3-day itinerary · $912 total"]));

  // The canvas slides in; the conversation narrows.
  const width = () => p.evaluate(() => Math.round(document.querySelector("#main > section.conv").getBoundingClientRect().width));
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
  const topAfter = await p.evaluate(() => Math.round(document.querySelector("#main > section.conv").getBoundingClientRect().top));
  check("the canvas slides in with the app's close", () => assert.deepEqual(canvas, { opening: true, close: true, title: "Plan the trip" }));
  check("the conversation narrows over time", () => assert.ok(midway < wide && narrow < midway, JSON.stringify({ wide, midway, narrow })));
  check("the conversation stays where it was when the canvas opens", () => assert.equal(topAfter, topBefore));
  check("wide canvas (task) takes ~54% of the stage", () => assert.ok(cols[3] > cols[1] && cols[3] <= 820, JSON.stringify(cols)));
  await p.evaluate(() => {
    const el = document.getElementById("detail");
    el.innerHTML = AuroraStage.shell({ kind: "needs", title: "Approvals", head: null, backBtn: "", body: "<p>Pending</p>" });
  });
  await p.waitForTimeout(600);
  const compactCols = await p.evaluate(() => getComputedStyle(document.querySelector(".shell")).gridTemplateColumns.split(" ").map(parseFloat));
  check("compact canvas (approvals/lists) stays between 380px and 460px", () => assert.ok(compactCols[3] >= 379 && compactCols[3] <= 461, JSON.stringify(compactCols)));

  // A new block of an answer streams in word by word; code stays as it is.
  const words = await p.evaluate(() => { AuroraStage.stream(document.querySelector(".answer-body")); return { words: [...document.querySelectorAll("#block .stream-word")].map(w => `${w.textContent}@${w.style.animationDelay}`), text: document.getElementById("block").textContent, code: document.querySelectorAll("#code .stream-word").length }; });
  check("a new block streams in word by word", () => assert.deepEqual(words, { words: ["The@0ms", "trip@34ms", "is@68ms", "ready@102ms", "to@136ms", "book.@170ms"], text: "The trip is ready to book.", code: 0 }));

  // Frosted glass.
  const blur = await p.evaluate(() => Object.fromEntries([["nav", "nav.side"], ["composer", ".say"], ["box", ".stage-tile"], ["canvas", ".stage-artifact"], ["column", "aside.detail"]].map(([k, q]) => [k, getComputedStyle(document.querySelector(q)).backdropFilter]).concat([["frame", getComputedStyle(document.querySelector(".shell"), "::before").backdropFilter]])));
  check("Aurora's islands blur what is behind them; the frame and the canvas's column do not", () => assert.deepEqual(blur, { nav: "blur(22px)", composer: "blur(22px)", frame: "none", box: "blur(20px)", canvas: "blur(20px)", column: "none" }));

  // Other styles have no bento and no dock.
  const plain = await p.evaluate(async () => { document.documentElement.dataset.style = "obsidian"; await AuroraStage.refresh(); return { on: AuroraStage.on(), bento: !!document.querySelector(".stage-bento"), dock: !!document.querySelector(".stage-dock") }; });
  check("another style has no bento and no dock", () => assert.deepEqual(plain, { on: false, bento: false, dock: false }));

  // A new file brings Files in.
  const f = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  f.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await f.setContent(page(true));
  await f.evaluate(() => AuroraStage.refresh());
  const file = await f.evaluate(() => [...document.querySelectorAll(".stage-tile")].map(t => `${t.dataset.tile}:${t.dataset.open}:${t.querySelector(".stage-tile-title").textContent}`));
  check("a new file brings Files in, opening the file", () => assert.deepEqual(file, ["files:file:f1:Itinerary.pdf"]));

  // Boxes come and go (decisions/2026-11-21-aurora-boxes-come-and-go.md).
  const keys = () => f.evaluate(() => [...document.querySelectorAll("section.conv > .stage-bento > .stage-tile:not(.stage-tile-leaving)")].map(t => `${t.dataset.tile}:${t.dataset.tone}`));
  // Time passes: the page's clock moves on by `ms`, and the stage looks again.
  const later = ms => f.evaluate(async ms => { const real = Date.now; Date.now = () => real() + ms; await AuroraStage.refresh(); }, ms);
  // Work that keeps running does not keep its box: it leaves on its own.
  await f.evaluate(() => { App.tasks = [{ task_id: "t9", thread: "c1", title: "Watch the inbox", status: "running", status_line: "Reading new mail" }]; return AuroraStage.refresh(); });
  const running = await keys();
  await f.waitForTimeout(15_400);
  const after = await keys();
  check("work that keeps running shows its box, which then leaves on its own", () => assert.deepEqual([running, after], [["live:run", "files:done"], []]));
  // A new status line is not a change; needing the person is, and stays.
  await f.evaluate(() => { App.tasks[0].status_line = "Reading more mail"; return AuroraStage.refresh(); });
  const sameWork = await keys();
  await f.evaluate(() => { App.tasks[0].status = "suspended"; FX.pending = [{ id: "i9", summary: "Reply to Dana?" }]; return AuroraStage.refresh(); });
  await later(60_000);
  const wanting = await keys();
  check("a new status line brings nothing back; work that needs the person, and a decision, stay", () => assert.deepEqual([sameWork, wanting], [[], ["live:needs", "approval:i9:needs"]]));
  // Every box has a Close: shown on hover, it puts the box away and opens nothing.
  await f.hover('.stage-tile[data-tile="approval:i9"]');
  await f.waitForTimeout(300);
  const closeShown = await f.evaluate(() => getComputedStyle(document.querySelector('.stage-tile[data-tile="approval:i9"] .stage-tile-close')).opacity);
  const opened = await f.evaluate(() => FX.opened.length);
  await f.click('.stage-tile[data-tile="approval:i9"] .stage-tile-close');
  await f.evaluate(() => AuroraStage.refresh());
  const closedOne = await f.evaluate(() => ({ keys: [...document.querySelectorAll("section.conv > .stage-bento > .stage-tile:not(.stage-tile-leaving)")].map(t => t.dataset.tile), opened: FX.opened.length, hash: location.hash }));
  check("every box has a Close, shown on hover; closed, the box goes and nothing opens", () => assert.deepEqual([closeShown, closedOne], ["1", { keys: ["live"], opened, hash: "" }]));
  // A closed decision stays away; the next one waiting takes the box.
  await f.evaluate(() => AuroraStage.refresh());
  const stillClosed = await keys();
  await f.evaluate(() => { FX.pending = [{ id: "i9", summary: "Reply to Dana?" }, { id: "i10", summary: "Pay the invoice?" }]; return AuroraStage.refresh(); });
  const another = await f.evaluate(() => [...document.querySelectorAll("section.conv > .stage-bento > .stage-tile:not(.stage-tile-leaving)")].map(t => `${t.dataset.tile}:${t.querySelector(".stage-tile-title").textContent}`));
  check("a closed decision stays away; the next one waiting takes the box", () => assert.deepEqual([stillClosed, another], [["live:needs"], ["live:Watch the inbox", "approval:i10:Pay the invoice?"]]));
  // A box the pointer is on stays, and leaves its time after the pointer goes.
  await f.mouse.move(5, 5);
  await f.evaluate(() => { FX.pending = []; App.tasks = [{ task_id: "t10", thread: "c1", title: "Sort the files", status: "running", status_line: "Sorting" }]; return AuroraStage.refresh(); });
  await f.hover('.stage-tile[data-tile="live"]');
  await later(60_000);
  const hovered = await keys();
  await f.mouse.move(5, 5);
  await f.waitForTimeout(100);
  const justLeft = await keys();
  await later(16_000);
  const leftAlone = await keys();
  check("a box the pointer is on stays, and leaves its time after the pointer goes", () => assert.deepEqual([hovered, justLeft, leftAlone], [["live:run"], ["live:run"], []]));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
