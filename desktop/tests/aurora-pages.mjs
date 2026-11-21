// Every screen scrolls and every page has a way out
// (decisions/2026-11-21-aurora-pages.md), in the real app, in Aurora, at
// 1280 × 720:
// - Explore, Apps, Teams and Trash scroll to their end with the wheel, and
//   nothing on them is cut off with no way to scroll to it;
// - an app far down Apps can be reached and opened, and its page has Back,
//   which goes back to Apps;
// - Explore, Teams and Trash have a close at their top right that stays as
//   the page scrolls and goes back to the conversation the person came from;
//   Apps keeps its own close and gets no second one; a conversation has none;
// - another style has no added close.
//
// MAGNUS_URL, MAGNUS_PASSWORD  the server and alice's password
// MAGNUS_CHROMIUM              Chrome to drive
// MAGNUS_SHOTS                 where to save the screenshots, if set
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
/** @type {string[]} */
const errors = [];
page.on("pageerror", e => errors.push(e.message));
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const shot = async (/** @type {string} */ name) => { if (shots) await page.screenshot({ path: join(shots, `pages-${name}.png`) }); };
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
const until = async (/** @type {() => boolean} */ fn, what = "UI condition") => {
  const end = Date.now() + 15000;
  while (Date.now() < end) { if (await page.evaluate(fn)) return; await page.waitForTimeout(40); }
  throw Error(`${what} timed out`);
};

/** What is cut off with no way to scroll to it, and the page's close. */
const look = () => page.evaluate(() => {
  const shown = (/** @type {Element} */ e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(e).visibility !== "hidden"; };
  const cut = [];
  for (const e of document.querySelectorAll("#main *, #main, aside.detail *")) {
    if (!shown(e) || e.closest("[hidden]")) continue;
    if (e.matches("textarea, canvas, svg, img, video, iframe, select, pre, code")) continue;
    const s = getComputedStyle(e);
    const hidden = e.scrollHeight - e.clientHeight;
    if (hidden > 24 && e.clientHeight > 80 && /hidden|clip/.test(s.overflowY)) cut.push(`${e.tagName.toLowerCase()}${e.id ? `#${e.id}` : ""}.${String(e.className).split(" ").slice(0, 2).join(".")} hides ${hidden}px`);
  }
  const close = /** @type {?HTMLElement} */ (document.querySelector("#main .aurora-page-close"));
  return { cut, close: close && shown(close) ? close.getAttribute("aria-label") : null, closes: document.querySelectorAll("#main .aurora-page-close").length, hash: location.hash };
});

/** Scrolls the page with the wheel to its end: whether it moved and its end is in view. */
const toTheEnd = async () => {
  const main = await page.locator("#main").boundingBox();
  if (!main) return { tall: false, moved: false, end: false, closeStays: false };
  const before = await page.evaluate(() => { const m = /** @type {HTMLElement} */ (document.getElementById("main")); return { top: m.scrollTop, tall: m.scrollHeight > m.clientHeight + 4 }; });
  await page.mouse.move(main.x + main.width / 2, main.y + main.height / 2);
  for (let i = 0; i < 30; i++) await page.mouse.wheel(0, 600);
  await page.waitForTimeout(400);
  return page.evaluate(b => {
    const m = /** @type {HTMLElement} */ (document.getElementById("main"));
    const box = m.getBoundingClientRect();
    const kids = [...m.querySelectorAll(":scope > * > *, :scope > *")].filter(e => e.getBoundingClientRect().height > 0);
    const last = Math.max(...kids.map(e => e.getBoundingClientRect().bottom));
    const close = m.querySelector(".aurora-page-close")?.getBoundingClientRect();
    return { tall: b.tall, moved: m.scrollTop > b.top, end: last <= box.bottom + 2, closeStays: !close || (close.top >= box.top - 16 && close.bottom <= box.top + 60) };
  }, before);
};

const go = async (/** @type {string} */ hash) => {
  await page.evaluate(h => { document.querySelectorAll("dialog[open]").forEach(d => /** @type {HTMLDialogElement} */ (d).close()); location.hash = h; }, hash);
  await page.waitForTimeout(1200);
};

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
  await page.evaluate(() => Theme.set({ style: "aurora", mode: "dark", motion: "reduced" }));
  await page.evaluate(() => document.querySelectorAll("dialog[open]").forEach(d => /** @type {HTMLDialogElement} */ (d).close()));
  // The conversation the person is in.
  await page.locator("[data-open^='thread:'], a[href^='#/chat/thread']").first().click().catch(() => {});
  await page.waitForTimeout(800);
  const talk = await page.evaluate(() => location.hash);
  let l = await look();
  check("a conversation has no page close", () => assert.equal(l.closes, 0));

  // Pages.
  for (const [hash, name, own] of /** @type {Array<[string, string, boolean]>} */ ([["#/explore", "explore", false], ["#/library", "apps", true], ["#/teams", "teams", false], ["#/trash", "trash", false]])) {
    await go(hash);
    l = await look();
    check(`${name}: nothing is cut off with no way to scroll to it`, () => assert.deepEqual(l.cut, []));
    if (own) check(`${name}: keeps its own close and gets no second`, () => assert.equal(l.closes, 0));
    else check(`${name}: has a close at its top right`, () => assert.ok(l.close?.startsWith("Close "), JSON.stringify(l)));
    await shot(`${name}-top`);
    const s = await toTheEnd();
    if (name === "explore" || name === "apps") check(`${name}: is taller than the window and scrolls to its end with the wheel`, () => assert.deepEqual([s.tall, s.moved, s.end], [true, true, true], JSON.stringify(s)));
    else check(`${name}: its end is in reach`, () => assert.equal(s.end, true, JSON.stringify(s)));
    if (!own) check(`${name}: its close stays at the top as it scrolls`, () => assert.equal(s.closeStays, true));
    await shot(`${name}-end`);
  }

  // An app far down Apps opens, and its page has Back to Apps.
  await go("#/library");
  const far = page.locator("#main a.lc-more").last();
  await far.scrollIntoViewIfNeeded();
  await far.click();
  await until(() => /^#\/library\/./.test(location.hash) && !!document.querySelector("#main .cp"), "the app's page");
  l = await look();
  await shot("app-open");
  check("an app's page has nothing cut off, and Back rather than a second close", () => assert.deepEqual([l.cut, l.closes], [[], 0]));
  await page.getByRole("link", { name: "Back to Apps" }).click();
  await page.waitForTimeout(800);
  const backed = await page.evaluate(() => location.hash);
  check("an app far down Apps opens, and Back goes back to Apps", () => assert.equal(backed, "#/library"));

  // The close goes back to the conversation.
  await go("#/explore");
  await page.locator("#main .aurora-page-close").click();
  await page.waitForTimeout(800);
  const backTo = await page.evaluate(() => ({ hash: location.hash, msg: !!document.getElementById("msg") }));
  check("Explore's close goes back to the conversation", () => assert.deepEqual(backTo, { hash: talk || backTo.hash, msg: true }));

  // Another style.
  await page.evaluate(() => Theme.set({ style: "obsidian" }));
  await go("#/explore");
  l = await look();
  check("another style has no added close", () => assert.equal(l.closes, 0));
  check("no page errors", () => assert.deepEqual(errors, []));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora pages: all checks passed");
