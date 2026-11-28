// Aurora says what Magnus is doing where the person is looking, as the
// mockup's agent demo does (decisions/2026-11-21-aurora-reasoning.md). In a
// real browser, with the app's sheets, aurora.js and aurora-reasoning.js on a
// page shaped like a conversation:
// - an answer being written shows the reasoning pill, not "Working on an
//   answer…", and Aurora's state reads Thinking, not Ready;
// - each thing the task does becomes a step: the pill's words follow it, the
//   step before ticks off, and it is the same pill throughout, also across a
//   repaint of the thread;
// - a task with a plan shows the plan's steps;
// - when the answer comes the pill sits above it, mint, says how many steps
//   in how long, and folds over time; a click opens it again;
// - above the composer a line says the state, in its colour; the composer
//   glows on focus and grows over time as the person types;
// - other styles have none of it.
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

// What the page gives the pill: the theme's stage test, and the task as the
// server says it now (TASK, changed by the test).
const stubs = `
var Theme = { stage: s => ["aurora", "obsidian-aurora"].includes(s) };
var TASK = { task_id: "t4", status: "running", created_at: new Date(Date.now() - 12000).toISOString(), status_line: null, plan: null };
var Server = { tasks: { get: async () => JSON.parse(JSON.stringify(TASK)) } };`;

const running = `<div class="entry agent" data-entry="a:t4" data-kind="turn"><div class="msg magnus"><div class="small faint">Working on an answer…</div><div class="pre" data-live="t4"></div></div></div>`;
const answered = `<div class="entry agent" data-entry="a:t4" data-kind="answer"><div class="msg magnus" data-answer data-task="t4"><div class="byline small faint">Magnus</div><div class="answer-body"><p>The flights are compared.</p></div></div></div>`;

const page = `<!doctype html><html data-style="aurora" data-mode="dark" data-motion="full" data-background="off"
  style="--bg:#05070E;--surface:#0E1424;--main:#0B1020;--fg:#E8EEF9;--fg2:#B5C0D4;--muted:#8592A8;--faint:#6B7891;--line-strong:rgba(255,255,255,.2);--warn:#F5B544;--ok:#34D399;--shadow:0 20px 50px rgba(0,0,0,.4);--aurora-glass:rgba(13,19,34,.62);--aurora-glass-strong:rgba(15,22,40,.82);--aurora-rim:rgba(186,215,255,.12);--aurora-beam-a:#38BDF8;--aurora-beam-b:#9B6DFF;--aurora-spot:rgba(79,157,255,.2);--aurora-mx:50%;--aurora-my:50%;--ease:cubic-bezier(.22,1,.36,1);--r-sm:10px;--r-md:16px;--r-lg:22px;--r-xl:26px;--r-pill:999px;--r-composer:22px">
<head><style>${ui("base.css")}${ui("layout.css")}${ui("chat.css")}${ui("aurora.css")}${ui("aurora-widgets.css")}</style></head>
<body><div class="shell"><main id="main"><section class="conv"><header class="conv-head"><div class="row"></div></header>
  <div class="thread" id="thread">${running}</div>
  <div class="composer-slot" id="slot"><form class="say"><textarea id="msg" rows="1"></textarea><div class="say-bar"></div></form></div>
</section></main></div>
<script>${stubs}</script><script>${ui("aurora.js")}</script><script>${ui("aurora-reasoning.js")}</script>
<script>const box = document.getElementById("msg"); box.addEventListener("input", () => { box.style.height = "auto"; box.style.height = box.scrollHeight + "px"; });</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page);
  // One read of the task, then the page's next frames.
  const tick = async fn => { if (fn) await p.evaluate(fn); await p.evaluate(() => AuroraReasoning.poll()); await p.waitForTimeout(120); };
  const pill = () => p.evaluate(() => {
    const el = document.querySelector(".aurora-reason");
    if (!el) return null;
    return { phase: el.dataset.phase, said: el.querySelector(".aurora-reason-summary").textContent, steps: [...el.querySelectorAll(".aurora-reason-steps > li")].map(li => `${li.dataset.state}:${li.firstElementChild.textContent}`), same: el === window.FIRST, under: el.parentElement.matches(".msg"), next: el.nextElementSibling?.className || "" };
  });
  await p.waitForTimeout(150);
  await tick();

  const first = await pill();
  const words = await p.evaluate(() => ({ hidden: getComputedStyle(document.querySelector(".msg > .small.faint")).display, state: document.documentElement.dataset.auroraState }));
  check("an answer being written shows the reasoning pill instead of a still line", () => assert.deepEqual([first?.phase, first?.said, words.hidden], ["thinking", "Thinking & planning…", "none"]));
  check("while it thinks, Aurora's state is Thinking, not Ready", () => assert.equal(words.state, "thinking"));
  await p.evaluate(() => { window.FIRST = document.querySelector(".aurora-reason"); });

  // It starts doing things: each is a step; the one before ticks off.
  await tick(() => { TASK.status_line = "Searching the web"; });
  const searching = await pill();
  await tick(() => { TASK.status_line = "Comparing flights"; });
  await p.waitForTimeout(350);
  const comparing = await pill();
  const ticked = await p.evaluate(() => getComputedStyle(document.querySelector('.aurora-reason-steps > li[data-state="done"] .aurora-reason-check')).opacity);
  const state = await p.evaluate(() => document.documentElement.dataset.auroraState);
  check("the pill's words follow what the task is doing", () => assert.deepEqual([searching.said, comparing.said], ["Searching the web", "Comparing flights"]));
  check("the step before ticks off and the one now is lit", () => assert.deepEqual([comparing.steps, ticked], [["done:Searching the web", "active:Comparing flights"], "1"]));
  check("it is the same pill throughout, changed in place", () => assert.deepEqual([searching.same, comparing.same], [true, true]));
  check("while it works, Aurora's state is Working", () => assert.equal(state, "executing"));

  // The thread repaints: the same pill is put back, its steps not replayed.
  await p.evaluate(r => { document.getElementById("thread").innerHTML = r; }, running);
  await p.waitForTimeout(80);
  const repainted = await pill();
  const replayed = await p.evaluate(() => document.getAnimations().filter(a => a.effect?.target?.closest?.(".aurora-reason-steps")).length);
  check("a repaint of the thread keeps the same pill and does not replay its steps", () => assert.deepEqual([repainted.same, repainted.steps.length, replayed], [true, 2, 0]));

  // A task with a plan shows the plan's steps.
  await tick(() => { TASK.plan = { units: [{ goal: "Find flights", status: "done" }, { goal: "Compare prices", status: "active" }, { goal: "Hold seats", status: "pending" }, { goal: "Old idea", status: "dropped" }] }; });
  const planned = await pill();
  check("a task with a plan shows the plan's steps, what it is on as the words", () => assert.deepEqual([planned.steps, planned.said], [["done:Find flights", "active:Compare prices", "todo:Hold seats"], "Compare prices"]));

  // The answer comes: the pill sits above it, mint, and folds over time.
  const openHeight = await p.evaluate(() => Math.round(document.querySelector(".aurora-reason-body").getBoundingClientRect().height));
  await p.evaluate(a => { document.getElementById("thread").innerHTML = a; }, answered);
  await p.waitForTimeout(140);
  const midway = await p.evaluate(() => Math.round(document.querySelector(".aurora-reason-body").getBoundingClientRect().height));
  await p.waitForTimeout(600);
  const done = await pill();
  const folded = await p.evaluate(() => Math.round(document.querySelector(".aurora-reason-body").getBoundingClientRect().height));
  const after = await p.evaluate(() => document.documentElement.dataset.auroraState);
  check("when the answer comes the pill sits above it, mint", () => assert.deepEqual([done.phase, done.same, done.next], ["done", true, "answer-body"]));
  check("it says how many steps in how long", () => assert.match(done.said, /^Completed 3 steps · 1\ds$/));
  check("it folds over time", () => assert.ok(openHeight > 0 && midway > 0 && midway < openHeight && folded === 0, JSON.stringify({ openHeight, midway, folded })));
  check("with the answer in, Aurora is Ready again", () => assert.equal(after, "idle"));
  await p.click(".aurora-reason-toggle");
  await p.waitForTimeout(500);
  const reopened = await p.evaluate(() => Math.round(document.querySelector(".aurora-reason-body").getBoundingClientRect().height));
  check("a click opens it again", () => assert.ok(reopened > 0, String(reopened)));

  // The composer: the state above it, a glow on focus, growth over time.
  const line = await p.evaluate(() => { const l = document.querySelector("#slot > .aurora-say-state"); return { text: l?.textContent, next: l?.nextElementSibling?.matches(".say"), color: l && getComputedStyle(l).color }; });
  // Work running elsewhere has its own card: under this conversation, whose
  // answer is in, the line stays at rest (installed app, test.427: "pong",
  // answered in 3s, and the line still said "Working across your apps…").
  await p.evaluate(() => { document.body.insertAdjacentHTML("beforeend", '<div id="elsewhere"><div class="tcard" data-status="running"></div></div>'); });
  await p.waitForTimeout(450);
  const elsewhere = await p.evaluate(() => ({ app: document.documentElement.dataset.auroraAvatarState, line: document.querySelector(".aurora-say-state")?.textContent }));
  await p.evaluate(() => { document.getElementById("elsewhere")?.remove(); });
  await p.waitForTimeout(300);
  check("work elsewhere does not say this answered conversation is working", () => assert.deepEqual([elsewhere.app, elsewhere.line], ["working", "Ready · standing by"], JSON.stringify(elsewhere)));
  await p.evaluate(() => { document.getElementById("thread").insertAdjacentHTML("beforeend", '<span class="st working"></span>'); });
  await p.waitForTimeout(450);
  const working = await p.evaluate(() => { const l = document.querySelector(".aurora-say-state"); return { text: l.textContent, color: getComputedStyle(l).color }; });
  check("above the composer a line says the state Magnus is in", () => assert.deepEqual([line.text, line.next, working.text], ["Ready · standing by", true, "Working…"]));
  check("the line takes the state's colour", () => assert.deepEqual([line.color !== working.color, working.color], [true, "rgb(56, 189, 248)"]));
  const glowBefore = await p.evaluate(() => getComputedStyle(document.getElementById("slot"), "::before").opacity);
  await p.focus("#msg");
  await p.waitForTimeout(150);
  const glowMid = await p.evaluate(() => Number(getComputedStyle(document.getElementById("slot"), "::before").opacity));
  await p.waitForTimeout(500);
  const glowAfter = await p.evaluate(() => getComputedStyle(document.getElementById("slot"), "::before").opacity);
  check("the composer glows on focus, over time", () => assert.ok(glowBefore === "0" && glowMid > 0 && glowMid < 1 && glowAfter === "1", JSON.stringify({ glowBefore, glowMid, glowAfter })));
  // Each height change the composer is given, as it is asked for.
  await p.evaluate(() => { window.GREW = []; const animate = Element.prototype.animate; Element.prototype.animate = function (k, o) { if (this.id === "msg") window.GREW.push(k.map(x => x.height)); return animate.call(this, k, o); }; });
  await p.keyboard.type("Line one");
  await p.keyboard.press("Shift+Enter");
  await p.keyboard.type("Line two");
  await p.keyboard.press("Shift+Enter");
  await p.waitForTimeout(100);
  const grew = await p.evaluate(() => window.GREW);
  check("the composer grows over time as the person types", () => assert.ok(grew.length > 0 && grew.every(([a, b]) => parseFloat(b) > parseFloat(a)), JSON.stringify(grew)));

  // Other styles have none of it.
  await p.evaluate(() => { document.documentElement.dataset.style = "obsidian"; });
  await p.waitForTimeout(150);
  const plain = await p.evaluate(() => ({ pill: !!document.querySelector(".aurora-reason"), line: !!document.querySelector(".aurora-say-state") }));
  check("another style has no pill and no state line", () => assert.deepEqual(plain, { pill: false, line: false }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
