// Aurora's motion plays once for what arrives, never for a repaint
// (decisions/2026-11-20-aurora-entrances-keyed.md). The app redraws a region
// by replacing its markup, so each live update makes new nodes; Aurora keyed
// its entrances on the node and replayed the conversation, the panel and
// every answer on each update, which the owner saw as the window flashing.
// In a real browser, with aurora.css and aurora.js on a page shaped like the
// app's:
// - repainting the same conversation and panel starts no Aurora animation;
// - an answer's new block is marked to enter before the browser first draws it;
// - a panel showing something new enters;
// - the hero arriving does not move the conversation's scroll;
// - focusing the composer does not change Aurora's state;
// - a card that starts working keeps its own surface.
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

const answer = blocks => `<div class="entry agent" data-entry="a1" data-kind="answer"><div class="answer-body">${blocks.map(b => `<p>${b}</p>`).join("")}</div></div>`;
const message = `<div class="entry me" data-entry="m1" data-kind="message"><div class="msg me">Plan the trip</div></div>`;
const card = status => `<div class="tcard" data-status="${status}"><span class="st">${status}</span></div>`;
const panel = key => `<div class="detail-head"></div><div class="detail-body" data-panel="${key}"><div class="layer"><h3>${key}</h3></div></div>`;
const page = `<!doctype html><html data-style="aurora" data-mode="dark" data-motion="full" data-background="off"
  style="--bg:#05070E;--surface:#0E1424;--aurora-glass:rgba(13,19,34,.62);--aurora-glass-strong:rgba(15,22,40,.82);--aurora-rim:rgba(186,215,255,.12);--aurora-beam-a:#38BDF8;--aurora-beam-b:#9B6DFF;--aurora-spot:rgba(79,157,255,.2);--ease:cubic-bezier(.22,1,.36,1);--r-md:16px">
<head><style>${ui("aurora.css")}</style></head>
<body><div class="shell">
  <nav class="side"><div class="brand"><span class="logo"><svg></svg></span></div></nav>
  <main id="main"><section class="conv"><div class="conv-head"><div class="row"></div></div>
    <div class="thread" id="thread" style="height:300px;overflow:auto">${message}${answer(["One", "Two"])}${card("done")}</div>
    <div id="composer"><div class="say"><textarea id="msg"></textarea></div></div></section></main>
  <aside class="detail" id="detail">${panel("activity")}</aside>
  <div class="computer-view"><span class="state-pill" data-pill="asleep">Asleep</span></div>
</div><script>${ui("aurora.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage();
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page);
  await p.evaluate(() => { window.__anims = []; document.addEventListener("animationstart", e => { if (e.animationName.startsWith("aurora") && e.animationName !== "auroraDotPulse" && e.animationName !== "auroraBeamSpin" && e.animationName !== "auroraSweep") window.__anims.push(`${e.animationName} on ${e.target.dataset.entry || e.target.className}`); }, true); });
  await p.waitForTimeout(400);
  const took = () => p.evaluate(() => { const a = window.__anims; window.__anims = []; return a; });
  await took();

  // The same conversation and panel, repainted as app.js paint does.
  await p.evaluate(({ thread, detail }) => {
    document.getElementById("thread").innerHTML = thread;
    document.getElementById("detail").innerHTML = detail;
  }, { thread: message + answer(["One", "Two"]) + card("done"), detail: panel("activity") });
  await p.waitForTimeout(600);
  const replayed = await took();
  check("a repaint of the same conversation and panel starts no entrance", () => assert.deepEqual(replayed, []));

  // An answer grows by a block: only that block enters, and it is marked in
  // the same task as the repaint, before the browser draws it.
  const marked = await p.evaluate(html => new Promise(done => {
    document.getElementById("thread").innerHTML = html;
    requestAnimationFrame(() => done([...document.querySelectorAll("[data-entry=a1] .answer-body > p")].map(n => n.classList.contains("aurora-stream-in"))));
  }), message + answer(["One", "Two", "Three"]) + card("done"));
  check("an answer's new block, and only it, is marked to enter before it is drawn", () => assert.deepEqual(marked, [false, false, true]));
  await p.waitForTimeout(600);
  await took();

  // The panel shows something else: it enters.
  await p.evaluate(detail => { document.getElementById("detail").innerHTML = detail; }, panel("files"));
  await p.waitForTimeout(600);
  const entered = await took();
  check("a panel showing something new enters", () => assert.ok(entered.some(a => a.startsWith("auroraViewIn")), JSON.stringify(entered)));

  // The hero arrives in a scrolled conversation.
  const scroll = await p.evaluate(async () => {
    const t = document.getElementById("thread");
    t.insertAdjacentHTML("afterbegin", `<div style="height:900px"></div>`);
    t.scrollTop = 200;
    t.insertAdjacentHTML("beforeend", `<div class="hello-head"><h1 class="hello-title">Hello</h1></div>`);
    await new Promise(r => setTimeout(r, 300));
    return t.scrollTop;
  });
  check("the hero arriving keeps the conversation where it was scrolled", () => assert.equal(scroll, 200));

  // Focusing the composer is not the agent thinking.
  const state = await p.evaluate(async () => { document.getElementById("msg").focus(); await new Promise(r => setTimeout(r, 300)); return document.documentElement.dataset.auroraState; });
  check("focusing the composer leaves Aurora's state idle", () => assert.equal(state, "idle"));

  // A card or computer that starts working keeps its surface; the beam is a
  // ring drawn over its edge.
  const surface = q => p.evaluate(q => { const s = getComputedStyle(document.querySelector(q)); return `${s.backgroundColor} ${s.backgroundImage}`; }, q);
  const cardBefore = await surface(".tcard"), screenBefore = await surface(".computer-view");
  await p.evaluate(() => { document.querySelector(".tcard").dataset.status = "running"; document.querySelector(".computer-view .state-pill").dataset.pill = "working"; });
  await p.waitForTimeout(300);
  const running = await p.evaluate(() => document.querySelector(".tcard").classList.contains("aurora-executing"));
  const ring = await p.evaluate(() => getComputedStyle(document.querySelector(".computer-view"), "::before").backgroundImage);
  const cardAfter = await surface(".tcard"), screenAfter = await surface(".computer-view");
  check("a card that starts working is marked working", () => assert.ok(running));
  check("a card that starts working keeps its own surface", () => assert.equal(cardAfter, cardBefore));
  check("a computer that starts working keeps its own surface", () => assert.equal(screenAfter, screenBefore));
  check("a computer that starts working wears the beam ring", () => assert.match(ring, /conic-gradient/));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
