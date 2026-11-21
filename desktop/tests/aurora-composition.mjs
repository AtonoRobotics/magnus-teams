// Aurora's composition (decisions/2026-11-21-aurora-composition.md), step A.
// In a real browser, on the page aurora-alerts.mjs checks
// (aurora-alerts-page.mjs):
// - one frame holds the conversation and the canvas: the conversation is
//   drawn on it, with no card of its own, and the canvas is a card inset in
//   it;
// - the boxes sit just above the input, after the thread, as wide as the
//   input, and stay there when the conversation is drawn again;
// - what comes to be told before the page is first drawn waits: once a
//   conversation is drawn, a decision leaves no pop-up and a message is a
//   box; once a page with no conversation is drawn, it is a pop-up;
// - another style has no frame.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { page, ask } from "./aurora-alerts-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
const clear = "rgba(0, 0, 0, 0)";
try {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("full"));
  await p.waitForTimeout(300);

  // A decision here (the canvas opens) and one from elsewhere (a box).
  await p.evaluate(([a, b]) => { FX.pending = [a, b]; drawThread(); }, [ask("i1", "c1", "Book the Lisbon flight for $412?"), ask("i2", "c9", "Pay the $912 deposit?")]);
  await p.waitForTimeout(900);
  const frame = await p.evaluate(() => {
    const shell = document.querySelector(".shell"), f = getComputedStyle(shell, "::before");
    const main = document.getElementById("main"), conv = main.querySelector("section.conv");
    const r = e => e.getBoundingClientRect(), s = r(shell), m = r(main), c = r(document.querySelector(".stage-artifact"));
    return {
      frame: f.content !== "none" && f.borderTopWidth === "1px" && f.backgroundColor !== "rgba(0, 0, 0, 0)",
      main: getComputedStyle(main).backgroundColor, conv: getComputedStyle(conv).backgroundColor, convBorder: getComputedStyle(conv).borderTopWidth,
      inset: { left: Math.round(c.left - m.right), right: Math.round(s.right - c.right), top: Math.round(c.top - m.top), bottom: Math.round(m.bottom - c.bottom) },
    };
  });
  check("one frame holds the work", () => assert.ok(frame.frame));
  check("the conversation is drawn on the frame, with no card of its own", () => assert.deepEqual([frame.main, frame.conv, frame.convBorder], ["rgba(0, 0, 0, 0)", "rgba(0, 0, 0, 0)", "0px"]));
  check("the canvas is a card inset in the frame", () => assert.ok(frame.inset.left > 0 && frame.inset.right >= 20 && frame.inset.top >= 9 && frame.inset.bottom >= 9, JSON.stringify(frame.inset)));

  const place = () => p.evaluate(() => {
    const b = document.querySelector(".stage-bento"), say = document.querySelector("#composer .say").getBoundingClientRect(), t = b.querySelector(".stage-tile").getBoundingClientRect();
    return { after: b.previousElementSibling?.id, before: b.nextElementSibling?.id, left: Math.round(t.left - say.left), aboveInput: b.getBoundingClientRect().bottom <= say.top + 1 };
  });
  const placed = await place();
  check("the boxes sit after the thread, just above the input", () => assert.deepEqual([placed.after, placed.before, placed.aboveInput], ["thread", "composer", true]));
  check("the boxes line up with the input", () => assert.ok(Math.abs(placed.left) <= 1, JSON.stringify(placed)));
  // The app draws the conversation again: the boxes keep their place.
  await p.evaluate(() => { const c = document.querySelector("#main > section.conv"); const again = c.cloneNode(true); again.querySelector(".stage-bento")?.remove(); c.replaceWith(again); });
  await p.waitForTimeout(500);
  const redrawn = await p.evaluate(() => ({ count: document.querySelectorAll(".stage-bento").length, after: document.querySelector(".stage-bento")?.previousElementSibling?.id, tiles: document.querySelectorAll(".stage-bento > .stage-tile:not(.stage-tile-leaving)").length }));
  check("drawn again, the conversation keeps its boxes above the input", () => assert.deepEqual(redrawn, { count: 1, after: "thread", tiles: 1 }));

  // Before the page is first drawn.
  const q = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  q.on("pageerror", e => failures.push(`early page error: ${e.message}`));
  await q.setContent(page("full"));
  await q.waitForTimeout(200);
  await q.evaluate(() => {
    window.__conv = document.querySelector("#main > section.conv");
    window.__conv.remove();
    Notices.show({ key: "inbox:i7", title: "A task needs you", body: "Magnus wants to send in Mail", system: "", href: "#/inbox" });
    Notices.show({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the deck?", system: "", href: "#/team/w1" });
  });
  const held = await q.evaluate(() => document.querySelectorAll("#notices .notice").length);
  await q.evaluate(() => document.getElementById("main").append(window.__conv));
  await q.waitForTimeout(500);
  const placedEarly = await q.evaluate(() => ({ popups: document.querySelectorAll("#notices .notice").length, box: document.querySelector('.stage-tile[data-tile="messages"] .stage-tile-title')?.textContent || null }));
  check("what comes before the page is drawn waits", () => assert.equal(held, 0));
  check("once a conversation is drawn, a decision leaves no pop-up and a message is a box", () => assert.deepEqual(placedEarly, { popups: 0, box: "Elena in #launch" }));
  await q.evaluate(() => {
    document.querySelector("#main > section.conv").remove();
    Notices.show({ key: "team:w1:n2", title: "Elena in #launch", body: "And the budget?", system: "", href: "#/team/w1" });
    const settings = document.createElement("section"); settings.className = "settings"; settings.textContent = "Settings";
    document.getElementById("main").append(settings);
  });
  await q.waitForTimeout(300);
  const noStage = await q.evaluate(() => document.querySelectorAll('#notices .notice[data-notice="team:w1:n2"]').length);
  check("once a page with no conversation is drawn, it is a pop-up", () => assert.equal(noStage, 1));

  // Another style: no frame.
  await p.evaluate(() => Theme.set({ style: "obsidian" }));
  await p.waitForTimeout(200);
  const plain = await p.evaluate(() => ({ frame: getComputedStyle(document.querySelector(".shell"), "::before").content, bento: document.querySelectorAll(".stage-bento").length }));
  check("another style has no frame and no boxes", () => assert.deepEqual(plain, { frame: "none", bento: 0 }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
