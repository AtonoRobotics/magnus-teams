// The stage beside a conversation fits what it holds, in the real app
// (tests/scratch/tests/stage_column.rs sets it up), at the owner's window
// size (1440 by 900), in Nova dark and light:
// - a sub-agent at work shows once, in the thread's own card: no stage tile
//   or row repeats it, and no tip card fills the stage;
// - a running task's tile sits in a column at most 360px wide, the
//   conversation keeps the rest, and the column is as tall as its tiles.
//
// MAGNUS_URL, MAGNUS_PASSWORD           the server and alice's password
// MAGNUS_SUB_THREAD, MAGNUS_TASK_THREAD  the conversation at work to measure
//                                        (one of them: another conversation's
//                                        running task shows on every stage)
// MAGNUS_SUB_GOAL                        the sub-agent's goal
// MAGNUS_CHROMIUM                        Chrome to drive; MAGNUS_SHOTS  optional
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
/** @type {string[]} */
const errors = [], failures = [];
page.on("pageerror", e => errors.push(e.message));
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
const T = 20_000;
const SIDE_MAX_PX = 360;
const goal = process.env.MAGNUS_SUB_GOAL || "";

/** The conversation, its thread and its stage, measured. */
const measure = () => page.evaluate(sub => {
  const box = (/** @type {?Element} */ e) => { if (!e) return null; const r = e.getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height), x: Math.round(r.left), y: Math.round(r.top) }; };
  const conv = document.querySelector("#main > section.conv");
  const bento = conv?.querySelector(":scope > .stage-bento") || null;
  const tiles = bento ? [...bento.querySelectorAll(":scope > .stage-tile:not(.stage-tile-leaving)")] : [];
  const tileBoxes = tiles.map(box);
  const tilesBottom = tileBoxes.length ? Math.max(...tileBoxes.map(b => (b?.y || 0) + (b?.h || 0))) : 0;
  const b = box(bento);
  return {
    mode: conv instanceof HTMLElement ? conv.dataset.stageMode || "" : "",
    conv: box(conv),
    thread: box(conv?.querySelector(":scope > .thread") || null),
    bento: b,
    tiles: tiles.map(t => t.querySelector(".stage-tile-title")?.textContent?.trim() || ""),
    subRows: bento ? bento.querySelectorAll('[data-key^="sub:"]').length : 0,
    repeats: tiles.some(t => (t.textContent || "").includes(sub.slice(0, 40))),
    tips: !!document.querySelector(".nova-wait-tips"),
    inline: document.querySelectorAll("#thread .sub-agent").length,
    belowTiles: b && tileBoxes.length ? b.y + b.h - tilesBottom : 0,
  };
}, goal);

const open = async (/** @type {string} */ thread, /** @type {string} */ ready) => {
  await page.evaluate(t => { location.hash = `#/chat/${encodeURIComponent(t)}`; }, thread);
  await page.locator(ready).first().waitFor({ timeout: T });
  await page.waitForTimeout(2500);
};

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor({ timeout: T });
  for (const mode of ["dark", "light"]) {
    await page.evaluate(m => Theme.set({ style: "nova", mode: m, motion: "reduced" }), mode);

    if (process.env.MAGNUS_SUB_THREAD) {
      await open(process.env.MAGNUS_SUB_THREAD, "#thread .sub-agent");
      const s = await measure();
      console.log(`${mode} sub-agent: ${JSON.stringify(s)}`);
      if (shots) await page.screenshot({ path: join(shots, `stage-${mode}-sub-agent.png`) });
      check(`${mode}: the sub-agent shows in the thread's own card`, () => assert.ok(s.inline >= 1, JSON.stringify(s)));
      check(`${mode}: no stage tile or row repeats the sub-agent`, () => assert.deepEqual([s.repeats, s.subRows], [false, 0], JSON.stringify(s)));
      check(`${mode}: no tip card fills the stage`, () => assert.equal(s.tips, false));
      check(`${mode}: with nothing else to show, the conversation has the whole width`, () => assert.ok(s.mode !== "composed" && s.thread && s.conv && s.thread.w >= s.conv.w - 2, JSON.stringify(s)));
    }

    if (process.env.MAGNUS_TASK_THREAD) {
      await open(process.env.MAGNUS_TASK_THREAD, "section.conv > .stage-bento > .stage-tile");
      const t = await measure();
      console.log(`${mode} task: ${JSON.stringify(t)}`);
      if (shots) await page.screenshot({ path: join(shots, `stage-${mode}-task.png`) });
      check(`${mode}: the running task has its tile beside the conversation`, () => assert.ok(t.mode === "composed" && t.tiles.length >= 1, JSON.stringify(t)));
      check(`${mode}: the side column is at most ${SIDE_MAX_PX}px wide`, () => assert.ok(t.bento && t.bento.w <= SIDE_MAX_PX, JSON.stringify(t)));
      check(`${mode}: the conversation keeps the rest of the width`, () => assert.ok(t.conv && t.thread && t.thread.w >= t.conv.w - SIDE_MAX_PX - 2, JSON.stringify(t)));
      check(`${mode}: the side column ends with its tiles, no empty height below them`, () => assert.ok(t.belowTiles <= 16, JSON.stringify(t)));
    }
  }
} catch (e) {
  failures.push(`the page did not get there: ${/** @type {Error} */ (e).message}`);
} finally {
  await browser.close();
}
if (errors.length) failures.push(`page errors: ${errors.join(" | ")}`);
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("stage column: ok");
