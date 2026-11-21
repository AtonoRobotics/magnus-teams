// Aurora's alerts move as the mockup's do (decisions/2026-11-21-aurora-alerts.md).
// In a real browser, with the app's theme, sheets, notices.js and
// aurora-alerts.js:
// - a pop-up is glass, with a dot for what it is about and a line for the
//   time it has left, held while the pointer is on it;
// - a pop-up dismissed slides out where it was and the one beside it glides
//   into the room left, instead of both moving in one frame; a toast too;
// - a decision already there when the page is drawn does not rise in; one
//   arriving does;
// - a decision answered shows the answer where it was, then folds away;
// - with reduced motion, and in other styles, things go at once as before.
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

const stubs = `
function $(q, r) { return (r || document).querySelector(q); }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function icon() { return "×"; }
var Magnus = { kind: "desktop", notifyPermission: () => "granted" };
function toast(msg, ms) { const t = document.createElement("div"); t.className = "toast"; t.textContent = msg; let box = $("#toasts"); if (!box) { box = document.createElement("div"); box.id = "toasts"; document.body.append(box); } box.append(t); setTimeout(() => t.remove(), ms || 3200); }`;

const card = (id, text) => `<article class="card item approval" data-id="${id}"><div class="spread"><strong>${text}</strong></div><div class="ask"><p class="ask-sentence">${text}</p><div class="row ask-answers"><button class="primary" data-item="${id}" data-decision="approve">Allow</button><button data-item="${id}" data-decision="reject">Decline</button></div></div></article>`;
const thread = ids => `<div class="entry agent" data-entry="a:t1" data-kind="tasks">${ids.map(([id, t]) => card(id, t)).join("")}</div>`;

const page = motion => `<!doctype html><html><head><style>${["base.css", "layout.css", "chat.css", "teams.css", "aurora.css", "aurora-widgets.css"].map(ui).join("\n")}
body { margin: 0; } #main { max-width: 720px; margin: 24px auto; }</style></head>
<body><script>${ui("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "${motion}", background: "off" });</script>
<main id="main"><div class="thread" id="thread">${thread([["i1", "Send the booking to Elena?"]])}</div></main>
<script>${stubs}</script><script>${ui("notices.js")}</script><script>${ui("aurora-alerts.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("full"));
  await p.waitForTimeout(150);

  // Two pop-ups: a message, then something that needs the person.
  await p.evaluate(() => {
    Notices.popup({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the deck?", system: "", href: "" });
    Notices.popup({ key: "inbox:i9", title: "A task needs you", body: "Send the booking?", system: "", href: "" });
  });
  await p.waitForTimeout(500);
  const look = await p.evaluate(() => {
    const [a, b] = document.querySelectorAll("#notices > .notice");
    const dot = el => getComputedStyle(el.querySelector(".notice-open > strong"), "::before").backgroundColor;
    const life = document.getAnimations().filter(x => x.animationName === "auroraNoticeLife").map(x => x.playState);
    return { blur: getComputedStyle(b).backdropFilter, message: dot(a), needs: dot(b), life };
  });
  check("a pop-up is glass, with a dot for what it is about", () => assert.deepEqual([look.blur, look.message], ["blur(20px)", "rgb(56, 189, 248)"]));
  check("a pop-up that needs the person has the amber dot", () => assert.notEqual(look.needs, look.message));
  check("each pop-up shows the time it has left", () => assert.deepEqual(look.life, ["running", "running"]));
  await p.hover('[data-notice="inbox:i9"]');
  await p.waitForTimeout(100);
  const held = await p.evaluate(() => document.getAnimations().filter(x => x.animationName === "auroraNoticeLife" && x.effect.target.dataset.notice === "inbox:i9").map(x => x.playState));
  check("the time is held while the pointer is on it", () => assert.deepEqual(held, ["paused"]));
  await p.mouse.move(10, 10);

  // The lower pop-up dismissed: it slides out where it was, the other glides down.
  const topBefore = await p.evaluate(() => document.querySelector('[data-notice="team:w1:n1"]').getBoundingClientRect().top);
  await p.click('[data-notice="inbox:i9"] .notice-close');
  const ghost = await p.evaluate(() => { const g = document.querySelector("#notices > .notice.aurora-leaving"); return g ? { key: g.dataset.notice, last: g === document.getElementById("notices").lastElementChild, inert: g.inert } : null; });
  await p.waitForTimeout(180);
  const topMid = await p.evaluate(() => document.querySelector('[data-notice="team:w1:n1"]:not(.aurora-leaving)').getBoundingClientRect().top);
  await p.waitForTimeout(450);
  const topAfter = await p.evaluate(() => document.querySelector('[data-notice="team:w1:n1"]:not(.aurora-leaving)').getBoundingClientRect().top);
  const gone = await p.evaluate(() => document.querySelectorAll("#notices > .aurora-leaving").length);
  check("a pop-up dismissed slides out from where it was", () => assert.deepEqual(ghost, { key: "inbox:i9", last: true, inert: true }));
  check("the pop-up beside it glides into the room left, over time", () => assert.ok(topBefore < topMid && topMid < topAfter, JSON.stringify({ topBefore, topMid, topAfter })));
  check("the pop-up that left is gone once it has slid out", () => assert.equal(gone, 0));

  // A toast leaves the same way.
  await p.evaluate(() => toast("Allowed.", 300));
  await p.waitForTimeout(380);
  const toastGhost = await p.evaluate(() => document.querySelectorAll("#toasts > .toast.aurora-leaving").length);
  await p.waitForTimeout(500);
  const toastGone = await p.evaluate(() => document.querySelectorAll("#toasts > .toast").length);
  check("a toast slides out when it goes, then is gone", () => assert.deepEqual([toastGhost, toastGone], [1, 0]));

  // Decisions: the one drawn with the page does not rise; one arriving does.
  const drawn = await p.evaluate(() => document.querySelector('[data-id="i1"]').classList.contains("aurora-alert-in"));
  await p.evaluate(t => { document.getElementById("thread").innerHTML = t; }, thread([["i1", "Send the booking to Elena?"], ["i2", "Pay the $912 deposit?"]]));
  await p.waitForTimeout(60);
  const arrived = await p.evaluate(() => ["i1", "i2"].map(id => document.querySelector(`[data-id="${id}"]`).classList.contains("aurora-alert-in")));
  check("a decision drawn with the page does not rise in; one arriving does", () => assert.deepEqual([drawn, arrived], [false, [false, true]]));

  // The first answered: the thread is redrawn without it, as app.js does.
  await p.evaluate(t => {
    document.querySelector('[data-id="i1"] [data-decision="approve"]').addEventListener("click", () => setTimeout(() => { document.getElementById("thread").innerHTML = t; }, 60));
  }, thread([["i2", "Pay the $912 deposit?"]]));
  await p.click('[data-id="i1"] [data-decision="approve"]');
  await p.waitForTimeout(160);
  const answered = await p.evaluate(() => { const g = document.querySelector(".card.aurora-leaving"); return g ? { id: g.dataset.id, first: g === g.parentElement.firstElementChild, said: g.querySelector(".aurora-confirmed-pill")?.textContent, buttons: g.querySelectorAll("button").length, h: Math.round(g.getBoundingClientRect().height) } : null; });
  await p.waitForTimeout(900);
  const folding = await p.evaluate(() => Math.round(document.querySelector(".card.aurora-leaving")?.getBoundingClientRect().height ?? -1));
  await p.waitForTimeout(700);
  const folded = await p.evaluate(() => document.querySelectorAll(".card.aurora-leaving").length);
  check("an answered decision shows the answer where it was", () => assert.deepEqual(answered && { id: answered.id, first: answered.first, said: answered.said, buttons: answered.buttons }, { id: "i1", first: true, said: "✓ Allowed", buttons: 0 }));
  check("then folds away over time", () => assert.ok(answered.h > 0 && folding > 0 && folding < answered.h && folded === 0, JSON.stringify({ h: answered.h, folding, folded })));

  // Reduced motion: things go at once.
  const r = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await r.setContent(page("reduced"));
  await r.waitForTimeout(100);
  await r.evaluate(() => Notices.popup({ key: "team:w1:n2", title: "Elena", body: "Hi", system: "", href: "" }));
  await r.click(".notice-close");
  const still = await r.evaluate(() => document.querySelectorAll("#notices > .notice").length);
  check("with reduced motion a pop-up goes at once", () => assert.equal(still, 0));

  // Another style: as before.
  await p.evaluate(() => Theme.set({ style: "obsidian" }));
  await p.evaluate(() => Notices.popup({ key: "team:w1:n3", title: "Elena", body: "Hi", system: "", href: "" }));
  await p.click('[data-notice="team:w1:n3"] .notice-close');
  const plain = await p.evaluate(() => document.querySelectorAll('#notices > [data-notice="team:w1:n3"], #notices > .aurora-leaving').length);
  check("another style's pop-up goes at once, as before", () => assert.equal(plain, 0));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
