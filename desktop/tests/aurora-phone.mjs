// Aurora on a phone (decisions/2026-11-21-aurora-phone.md), in the real app
// with a real computer, at 390 × 844:
// - nothing in the conversation's head runs off the screen: the computer
//   chip is wholly on it, still named for its state;
// - the computer's card keeps what is in it off its frame;
// - the same holds turned sideways, and at desktop width the chip still
//   says its state in words.
//
// MAGNUS_URL, MAGNUS_PASSWORD  the server and alice's password
// MAGNUS_CHROMIUM              Chrome to drive
// MAGNUS_COMPUTER              alice's computer
// MAGNUS_SHOTS                 where to save the screenshots, if set
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
const page = await ctx.newPage();
/** @type {string[]} */
const errors = [];
page.on("pageerror", e => errors.push(e.message));
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const shot = async (/** @type {string} */ name) => { if (shots) await page.screenshot({ path: join(shots, `phone-${name}.png`) }); };
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
const computer = process.env.MAGNUS_COMPUTER || "";

/** What in the conversation's head is off the screen, and the computer chip. */
const head = () => page.evaluate(() => {
  const vw = document.documentElement.clientWidth;
  const off = [...document.querySelectorAll("#conv-head, #conv-head *")].filter(e => { const r = e.getBoundingClientRect(); return r.width > 1 && r.height > 1 && (r.right > vw + 0.5 || r.left < -0.5); })
    .map(e => `${e.tagName.toLowerCase()}.${String(e.className).split(" ")[0]} ${Math.round(e.getBoundingClientRect().left)}..${Math.round(e.getBoundingClientRect().right)}`);
  const c = document.querySelector("#conv-head .computer-chip"), r = c?.getBoundingClientRect();
  const words = c?.querySelector(".computer-chip-words")?.getBoundingClientRect();
  return { off, chip: !!c, inside: !!r && r.left >= 0 && r.right <= vw, name: (c?.textContent || "").trim(), wordsShown: !!words && words.width > 2 };
});

/** Explore's cards whose words or buttons leave the card, run off the screen or sit on the card's label, and whether the page runs off the screen. */
const explore = () => page.evaluate(() => {
  const vw = document.documentElement.clientWidth;
  const bad = [...document.querySelectorAll("#main .explore-grid > .explore-card")].flatMap(c => {
    const f = c.getBoundingClientRect(), name = /** @type {HTMLElement} */ (c).dataset.playbook;
    const parts = [...c.children].filter(e => !e.classList.contains("card-media") && e.getBoundingClientRect().height > 0);
    const out = parts.filter(e => { const r = e.getBoundingClientRect(); return r.top < f.top + 8 || r.bottom > f.bottom - 8 || r.left < f.left + 8 || r.right > f.right - 8; })
      .map(e => `${name}: ${e.tagName.toLowerCase()}.${String(e.className).split(" ")[0]} leaves the card`);
    if (f.left < -0.5 || f.right > vw + 0.5) out.push(`${name}: the card runs off the screen ${Math.round(f.left)}..${Math.round(f.right)}`);
    const head = c.querySelector(".explore-card-head")?.getBoundingClientRect();
    const words = parts.filter(e => !e.classList.contains("explore-card-head"));
    if (head && words.some(e => e.getBoundingClientRect().top < head.bottom)) out.push(`${name}: the words sit on the card's label`);
    return out;
  });
  return { bad, off: document.documentElement.scrollWidth - document.documentElement.clientWidth };
});

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
  await page.evaluate(() => { Theme.set({ style: "aurora", mode: "dark", motion: "reduced" }); document.querySelectorAll("dialog[open]").forEach(d => /** @type {HTMLDialogElement} */ (d).close()); });
  await page.locator("#conv-head .computer-chip").waitFor({ timeout: 20000 });
  await page.waitForTimeout(800);
  let h = await head();
  await shot("head");
  check("nothing in the conversation's head runs off the screen", () => assert.deepEqual(h.off, []));
  check("the computer chip is wholly on the screen, named for its state", () => assert.ok(h.chip && h.inside && /computer/i.test(h.name), JSON.stringify(h)));

  // The computer's card.
  await page.evaluate(id => { location.hash = `#/computer/${encodeURIComponent(id)}`; }, computer);
  await page.locator("#main .computer-view .state-pill").waitFor({ timeout: 20000 });
  await page.waitForTimeout(800);
  const card = await page.evaluate(() => {
    const v = /** @type {HTMLElement} */ (document.querySelector("#main .computer-view")), f = v.getBoundingClientRect();
    const near = [...v.querySelectorAll(".state-pill, .control-strip button, .screen")].map(e => { const r = e.getBoundingClientRect(); return Math.min(r.left - f.left, f.right - r.right, r.top - f.top); });
    return { least: Math.round(Math.min(...near)), off: document.documentElement.scrollWidth - document.documentElement.clientWidth };
  });
  await shot("computer");
  check("the computer's card keeps what is in it off its frame", () => assert.ok(card.least >= 8, JSON.stringify(card)));
  check("the computer's page does not run off the screen", () => assert.equal(card.off, 0));

  // Explore's cards, closed and with a playbook's steps open.
  await page.evaluate(() => { location.hash = "#/explore"; });
  await page.locator("#main .explore-grid > .explore-card").first().waitFor({ timeout: 20000 });
  await page.waitForTimeout(800);
  let ex = await explore();
  await page.locator("#main .explore-grid > .explore-card").first().scrollIntoViewIfNeeded();
  await shot("explore");
  check("each Explore card keeps what is in it inside its frame", () => assert.deepEqual(ex.bad, []));
  check("Explore does not run off the screen", () => assert.equal(ex.off, 0));
  await page.locator("#main .explore-grid > .explore-card").nth(1).locator(".explore-steps > summary").click();
  await page.waitForTimeout(400);
  ex = await explore();
  await shot("explore-steps");
  check("a card with its steps open grows to hold them", () => assert.deepEqual(ex.bad, []));
  await page.setViewportSize({ width: 844, height: 390 });
  await page.waitForTimeout(800);
  ex = await explore();
  await shot("explore-sideways");
  check("turned sideways, each Explore card keeps what is in it inside its frame", () => assert.deepEqual(ex.bad, []));
  check("turned sideways, Explore does not run off the screen", () => assert.equal(ex.off, 0));
  await page.setViewportSize({ width: 390, height: 844 });

  // Sideways, and at desktop width.
  await page.evaluate(() => { location.hash = "#/chat/new"; });
  await page.setViewportSize({ width: 844, height: 390 });
  await page.locator("#conv-head .computer-chip").waitFor({ timeout: 20000 });
  await page.waitForTimeout(800);
  h = await head();
  await shot("sideways");
  check("turned sideways, nothing in the head runs off the screen", () => assert.deepEqual(h.off, []));
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.waitForTimeout(800);
  h = await head();
  check("at desktop width the chip still says its state in words", () => assert.ok(h.wordsShown && h.inside, JSON.stringify(h)));
  check("no page errors", () => assert.deepEqual(errors, []));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora phone: all checks passed");
